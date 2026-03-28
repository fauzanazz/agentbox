use std::collections::{HashMap, VecDeque};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::{Mutex, RwLock};

use serde::Serialize;

use crate::{
    config::{GuestConfig, PoolConfig},
    error::{AgentBoxError, Result},
    sandbox::*,
    vm::VmManager,
};

#[derive(Debug, Serialize)]
pub struct PoolStatus {
    pub warm_vms: usize,
    pub active_sandboxes: usize,
    pub config: PoolStatusConfig,
}

#[derive(Debug, Serialize)]
pub struct PoolStatusConfig {
    pub min_size: usize,
    pub max_size: usize,
    pub idle_timeout_secs: u64,
}

struct PooledSandbox {
    sandbox: Sandbox,
    pooled_at: Instant,
}

pub struct Pool {
    config: PoolConfig,
    guest_config: GuestConfig,
    vm_manager: Arc<VmManager>,
    available: Arc<Mutex<VecDeque<PooledSandbox>>>,
    active: Arc<RwLock<HashMap<SandboxId, SandboxInfo>>>,
}

impl Pool {
    pub fn new(config: PoolConfig, guest_config: GuestConfig, vm_manager: Arc<VmManager>) -> Self {
        Self {
            config,
            guest_config,
            vm_manager,
            available: Arc::new(Mutex::new(VecDeque::new())),
            active: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    pub async fn start(&self) -> Result<tokio::task::JoinHandle<()>> {
        let available = self.available.clone();
        let active = self.active.clone();
        let vm_manager = self.vm_manager.clone();
        let config = self.config.clone();
        let guest_config = self.guest_config.clone();

        let handle = tokio::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_secs(1));
            loop {
                interval.tick().await;

                // 1. Evict idle VMs
                let idle_timeout = Duration::from_secs(config.idle_timeout_secs);
                let to_destroy = {
                    let mut avail = available.lock().await;
                    let mut expired = Vec::new();
                    let mut i = 0;
                    while i < avail.len() {
                        if avail[i].pooled_at.elapsed() > idle_timeout {
                            if let Some(ps) = avail.remove(i) {
                                expired.push(ps);
                            }
                        } else {
                            i += 1;
                        }
                    }
                    expired
                };
                for ps in to_destroy {
                    let _ = vm_manager.destroy(ps.sandbox.into_vm()).await;
                }

                // 2. Replenish if needed
                let avail_count = available.lock().await.len();
                let active_count = active.read().await.len();
                if avail_count < config.min_size && avail_count + active_count < config.max_size {
                    let defaults = SandboxConfig {
                        memory_mb: 2048,
                        vcpus: 2,
                        network: false,
                        timeout_secs: 3600,
                    };
                    match vm_manager.create_from_snapshot(&defaults).await {
                        Ok(vm) => {
                            let sandbox = Sandbox::new(vm, defaults, &guest_config);
                            let ping_timeout = Duration::from_millis(guest_config.ping_timeout_ms);
                            let ready = tokio::time::timeout(ping_timeout, async {
                                loop {
                                    if sandbox.is_alive().await {
                                        return true;
                                    }
                                    tokio::time::sleep(Duration::from_millis(100)).await;
                                }
                            })
                            .await
                            .unwrap_or(false);

                            if ready {
                                available.lock().await.push_back(PooledSandbox {
                                    sandbox,
                                    pooled_at: Instant::now(),
                                });
                                tracing::debug!("Pool replenished a warm VM");
                            } else {
                                tracing::warn!("New VM guest agent not ready, discarding");
                                let _ = vm_manager.destroy(sandbox.into_vm()).await;
                            }
                        }
                        Err(e) => {
                            tracing::error!("Pool replenishment failed: {e}");
                        }
                    }
                }
            }
        });

        Ok(handle)
    }

    pub async fn claim(&self, config: SandboxConfig) -> Result<Sandbox> {
        // Fast path: pop a warm VM that matches the requested network config
        let pooled = {
            let mut avail = self.available.lock().await;
            let idx = avail
                .iter()
                .position(|ps| ps.sandbox.config.network == config.network);
            idx.and_then(|i| avail.remove(i).map(|ps| ps.sandbox))
        };

        let sandbox = if let Some(sb) = pooled {
            tracing::debug!(sandbox_id = %sb.id(), network = config.network, "Claimed warm sandbox from pool");
            sb
        } else {
            // Slow path: on-demand creation
            let active_count = self.active.read().await.len();
            let avail_count = self.available.lock().await.len();
            if active_count + avail_count >= self.config.max_size {
                return Err(AgentBoxError::PoolExhausted);
            }

            tracing::info!(network = config.network, "Pool miss, creating sandbox on demand");
            let vm = self.vm_manager.create_from_snapshot(&config).await?;
            let sb = Sandbox::new(vm, config.clone(), &self.guest_config);

            // Wait for guest agent readiness
            let ping_timeout = Duration::from_millis(self.guest_config.ping_timeout_ms);
            let ready = tokio::time::timeout(ping_timeout, async {
                loop {
                    if sb.is_alive().await {
                        return true;
                    }
                    tokio::time::sleep(Duration::from_millis(100)).await;
                }
            })
            .await
            .unwrap_or(false);

            if !ready {
                let _ = self.vm_manager.destroy(sb.into_vm()).await;
                return Err(AgentBoxError::Timeout(
                    "Guest agent not ready within timeout".into(),
                ));
            }

            // Configure guest networking for fresh-booted VMs with TAP
            if config.network {
                if let Err(e) = sb.setup_guest_network().await {
                    let _ = self.vm_manager.destroy(sb.into_vm()).await;
                    return Err(e);
                }
            }

            sb
        };

        // Register as active
        let info = sandbox.info();
        self.active.write().await.insert(sandbox.id().clone(), info);
        Ok(sandbox)
    }

    pub async fn release(&self, sandbox: Sandbox) -> Result<()> {
        let id = sandbox.id().clone();
        self.active.write().await.remove(&id);
        let vm = sandbox.into_vm();
        self.vm_manager.destroy(vm).await?;
        tracing::info!(sandbox_id = %id, "Sandbox released and destroyed");
        Ok(())
    }

    /// Get current pool status. Non-blocking — uses try_lock/try_read to avoid contention.
    pub fn status(&self) -> PoolStatus {
        let warm_vms = self
            .available
            .try_lock()
            .map(|guard| guard.len())
            .unwrap_or(0);
        let active_sandboxes = self.active.try_read().map(|guard| guard.len()).unwrap_or(0);
        PoolStatus {
            warm_vms,
            active_sandboxes,
            config: PoolStatusConfig {
                min_size: self.config.min_size,
                max_size: self.config.max_size,
                idle_timeout_secs: self.config.idle_timeout_secs,
            },
        }
    }

    /// List active sandboxes. Synchronous — uses try_read to avoid blocking.
    pub fn list_active(&self) -> Vec<SandboxInfo> {
        self.active
            .try_read()
            .map(|guard| guard.values().cloned().collect())
            .unwrap_or_default()
    }

    pub async fn shutdown(&self) -> Result<()> {
        let to_destroy: Vec<_> = {
            let mut avail = self.available.lock().await;
            avail.drain(..).map(|ps| ps.sandbox).collect()
        };
        for sb in to_destroy {
            let _ = self.vm_manager.destroy(sb.into_vm()).await;
        }
        let active_count = self.active.read().await.len();
        if active_count > 0 {
            tracing::warn!("{active_count} sandboxes still active during shutdown");
        }
        tracing::info!("Pool shutdown complete");
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{GuestConfig, PoolConfig, VmConfig};

    fn test_pool() -> Pool {
        let vm_manager = Arc::new(VmManager::new(VmConfig::default()));
        Pool::new(PoolConfig::default(), GuestConfig::default(), vm_manager)
    }

    #[test]
    fn test_pool_new() {
        let pool = test_pool();
        assert_eq!(pool.config.min_size, 2);
        assert_eq!(pool.config.max_size, 10);
    }

    #[test]
    fn test_pool_list_active_empty() {
        let pool = test_pool();
        assert!(pool.list_active().is_empty());
    }

    #[test]
    fn test_pool_status() {
        let pool = test_pool();
        let status = pool.status();
        assert_eq!(status.warm_vms, 0);
        assert_eq!(status.active_sandboxes, 0);
        assert_eq!(status.config.min_size, 2);
        assert_eq!(status.config.max_size, 10);
    }

    #[tokio::test]
    async fn test_pool_claim_exhausted_when_max_size_zero() {
        let pool_config = PoolConfig {
            min_size: 0,
            max_size: 0,
            idle_timeout_secs: 3600,
        };
        let vm_manager = Arc::new(VmManager::new(VmConfig::default()));
        let pool = Pool::new(pool_config, GuestConfig::default(), vm_manager);

        let config = SandboxConfig {
            memory_mb: 512,
            vcpus: 1,
            network: false,
            timeout_secs: 60,
        };
        let result = pool.claim(config).await;
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), AgentBoxError::PoolExhausted));
    }

    #[tokio::test]
    async fn test_pool_shutdown_on_empty_pool() {
        let pool = test_pool();
        let result = pool.shutdown().await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_pool_claim_on_demand_fails_with_bad_config() {
        // Use max_size > 0 so we enter the on-demand creation path
        let pool_config = PoolConfig {
            min_size: 0,
            max_size: 5,
            idle_timeout_secs: 3600,
        };
        // VmManager with default (nonexistent) paths should fail during create_from_snapshot
        let vm_manager = Arc::new(VmManager::new(VmConfig::default()));
        let pool = Pool::new(pool_config, GuestConfig::default(), vm_manager);

        let config = SandboxConfig {
            memory_mb: 512,
            vcpus: 1,
            network: false,
            timeout_secs: 60,
        };
        let result = pool.claim(config).await;
        assert!(result.is_err());
    }
}
