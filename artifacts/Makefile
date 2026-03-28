ARCH ?= $(shell uname -m)
OUTPUT_DIR := output/$(ARCH)
KERNEL_VERSION := 4.14.336
ALPINE_VERSION := 3.20
ROOTFS_SIZE_MB := 512

.PHONY: all kernel rootfs guest-agent snapshot templates template-snapshots clean

all: kernel rootfs guest-agent snapshot

$(OUTPUT_DIR):
	mkdir -p $(OUTPUT_DIR)

# === Kernel ===
kernel: $(OUTPUT_DIR)/vmlinux

$(OUTPUT_DIR)/vmlinux: | $(OUTPUT_DIR)
	./kernel/build.sh $(KERNEL_VERSION) $(ARCH) $(OUTPUT_DIR)

# === Rootfs ===
rootfs: $(OUTPUT_DIR)/rootfs.ext4

$(OUTPUT_DIR)/rootfs.ext4: $(OUTPUT_DIR)/guest-agent | $(OUTPUT_DIR)
	./rootfs/build.sh $(ALPINE_VERSION) $(ARCH) $(OUTPUT_DIR) $(ROOTFS_SIZE_MB)

# === Guest Agent ===
guest-agent: $(OUTPUT_DIR)/guest-agent

$(OUTPUT_DIR)/guest-agent: | $(OUTPUT_DIR)
	./guest-agent/build.sh $(ARCH) $(OUTPUT_DIR)

# === Snapshot ===
snapshot: $(OUTPUT_DIR)/snapshot/vmstate.bin

$(OUTPUT_DIR)/snapshot/vmstate.bin: $(OUTPUT_DIR)/vmlinux $(OUTPUT_DIR)/rootfs.ext4 | $(OUTPUT_DIR)
	./snapshot/bake.sh $(ARCH) $(OUTPUT_DIR)

# === Templates ===
TEMPLATES := node

templates: $(foreach t,$(TEMPLATES),$(OUTPUT_DIR)/rootfs-$(t).ext4)

$(OUTPUT_DIR)/rootfs-%.ext4: $(OUTPUT_DIR)/rootfs.ext4
	./templates/build-template.sh $* $(ARCH) $(OUTPUT_DIR)

# === Template Snapshots ===
template-snapshots: $(foreach t,$(TEMPLATES),$(OUTPUT_DIR)/snapshot-$(t)/vmstate.bin)

$(OUTPUT_DIR)/snapshot-%/vmstate.bin: $(OUTPUT_DIR)/vmlinux $(OUTPUT_DIR)/rootfs-%.ext4
	./snapshot/bake.sh $(ARCH) $(OUTPUT_DIR) --rootfs $*

# === Clean ===
clean:
	rm -rf output/
