KERNEL_BIN := kernel/target/x86_64-unknown-none/release/fabric-kernel
ISO := fabric-os.iso
LIMINE_DIR := $(HOME)/limine
OVMF_CODE := /usr/share/OVMF/OVMF_CODE_4M.fd
OVMF_VARS := /usr/share/OVMF/OVMF_VARS_4M.fd
OVMF_VARS_COPY := ovmf_vars.fd

.PHONY: all build iso run run-debug clean ocrb

all: run

build:
	cd kernel && RUSTFLAGS='-Awarnings' cargo build --release

iso: build
	# Create ISO directory structure
	mkdir -p iso_root/boot/limine
	mkdir -p iso_root/EFI/BOOT
	# Copy kernel binary
	cp $(KERNEL_BIN) iso_root/boot/fabric-kernel
	# Copy Limine files
	cp $(LIMINE_DIR)/BOOTX64.EFI iso_root/EFI/BOOT/
	cp $(LIMINE_DIR)/limine-bios.sys iso_root/boot/limine/ 2>/dev/null || true
	cp $(LIMINE_DIR)/limine-bios-cd.bin iso_root/boot/limine/ 2>/dev/null || true
	cp $(LIMINE_DIR)/limine-uefi-cd.bin iso_root/boot/limine/ 2>/dev/null || true
	# Create ISO
	xorriso -as mkisofs \
		-b boot/limine/limine-bios-cd.bin \
		-no-emul-boot -boot-load-size 4 -boot-info-table \
		--efi-boot boot/limine/limine-uefi-cd.bin \
		-efi-boot-part --efi-boot-image --protective-msdos-label \
		iso_root -o $(ISO)
	# Install Limine
	$(LIMINE_DIR)/limine bios-install $(ISO) 2>/dev/null || true

run: iso
	cp -n $(OVMF_VARS) $(OVMF_VARS_COPY) 2>/dev/null || true
	qemu-system-x86_64 \
		-drive if=pflash,format=raw,readonly=on,file=$(OVMF_CODE) \
		-drive if=pflash,format=raw,file=$(OVMF_VARS_COPY) \
		-cdrom $(ISO) \
		-serial stdio \
		-m 256M \
		-no-reboot \
		-no-shutdown \
		-display none

run-debug: iso
	cp -n $(OVMF_VARS) $(OVMF_VARS_COPY) 2>/dev/null || true
	qemu-system-x86_64 \
		-drive if=pflash,format=raw,readonly=on,file=$(OVMF_CODE) \
		-drive if=pflash,format=raw,file=$(OVMF_VARS_COPY) \
		-cdrom $(ISO) \
		-serial stdio \
		-m 256M \
		-no-reboot \
		-no-shutdown \
		-s -S

ocrb: iso
	cp -n $(OVMF_VARS) $(OVMF_VARS_COPY) 2>/dev/null || true
	qemu-system-x86_64 \
		-drive if=pflash,format=raw,readonly=on,file=$(OVMF_CODE) \
		-drive if=pflash,format=raw,file=$(OVMF_VARS_COPY) \
		-cdrom $(ISO) \
		-serial stdio \
		-m 256M \
		-no-reboot \
		-no-shutdown \
		-display none

clean:
	cd kernel && cargo clean
	rm -rf iso_root/boot/fabric-kernel
	rm -f $(ISO) $(OVMF_VARS_COPY)
