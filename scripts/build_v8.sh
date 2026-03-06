#!/bin/bash
# V8 Cross-Compilation Script for FabricOS
# Target: x86_64-unknown-none (bare metal)
# Uses custom platform: src/v8_platform

set -e

# Configuration
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(dirname "$SCRIPT_DIR")"
V8_DIR="$PROJECT_ROOT/vendor/v8"
BUILD_DIR="$PROJECT_ROOT/build/v8"
V8_PLATFORM_DIR="$PROJECT_ROOT/kernel/src/v8_platform"

# Target configuration
TARGET_ARCH="x64"
TARGET_OS="fabricos"

echo "=== V8 Build for FabricOS ==="
echo "Project root: $PROJECT_ROOT"
echo "V8 source: $V8_DIR"
echo "Build output: $BUILD_DIR"

# Create build directory
mkdir -p "$BUILD_DIR"

# Check for depot_tools
if [ -z "$DEPOT_TOOLS" ]; then
    DEPOT_TOOLS="$PROJECT_ROOT/vendor/depot_tools"
    if [ ! -d "$DEPOT_TOOLS" ]; then
        echo "Cloning depot_tools..."
        git clone https://chromium.googlesource.com/chromium/tools/depot_tools.git "$DEPOT_TOOLS"
    fi
fi

export PATH="$DEPOT_TOOLS:$PATH"

# Clone V8 if not present
if [ ! -d "$V8_DIR" ]; then
    echo "Fetching V8 source..."
    mkdir -p "$V8_DIR"
    cd "$V8_DIR"
    fetch v8
    cd v8
    # Checkout a stable version (12.4.x)
    git checkout 12.4.254.19
    gclient sync
fi

cd "$V8_DIR/v8"

echo "=== Applying FabricOS patches ==="

# Create FabricOS platform directory
mkdir -p src/base/platform/fabricos

# Copy our platform implementation
echo "Installing FabricOS platform files..."
cp -r "$V8_PLATFORM_DIR"/* src/base/platform/fabricos/ 2>/dev/null || true

echo "=== Generating build configuration ==="

# Generate GN args for FabricOS
cat > out/fabricos/args.gn << 'EOF'
# V8 Build Configuration for FabricOS
is_debug = false
is_official_build = true

# Target platform
target_cpu = "x64"
# Custom OS - we use linux as base but with heavy modifications
# V8 doesn't have native FabricOS support, so we use the embedded pattern

# Build type
is_component_build = false
is_static = true

# Features for bare metal
v8_enable_i18n_support = false
v8_enable_gdbjit = false
v8_use_snapshot = true
v8_use_external_startup_data = false

# No standard library
use_custom_libcxx = false
use_sysroot = false

# Disable features not available in bare metal
v8_enable_jit = true
v8_enable_pointer_compression = false

# Optimizations
v8_enable_fast_maths = true
v8_enable_future = true

# Logging
v8_enable_logging_and_profiling = false

# Snapshot
v8_use_snapshot = true
v8_embedded_builtins = true

# Custom definitions
extra_cflags = [
    "-DV8_OS_FABRICOS",
    "-DV8_TARGET_OS_FABRICOS",
    "-nostdlib",
    "-fno-exceptions",
    "-fno-rtti",
    "-ffreestanding",
    "-m64",
    "-march=x86-64",
    "-fno-stack-protector",
]

extra_ldflags = [
    "-nostdlib",
    "-static",
]
EOF

# Create output directory
mkdir -p out/fabricos

# Generate build files
echo "Running gn gen..."
gn gen out/fabricos

echo "=== Building V8 ==="
echo "This may take 30-60 minutes..."

# Build V8 static libraries
ninja -C out/fabricos v8_monolith

# Collect outputs
echo "=== Collecting build artifacts ==="

mkdir -p "$BUILD_DIR"

# Copy static libraries
cp out/fabricos/obj/libv8_monolith.a "$BUILD_DIR/libv8.a" 2>/dev/null || \
cp out/fabricos/obj/v8/libv8_snapshot.a "$BUILD_DIR/libv8_snapshot.a" 2>/dev/null || \
cp out/fabricos/obj/v8_snapshot.a "$BUILD_DIR/" 2>/dev/null || true

# Copy snapshot blob
cp out/fabricos/snapshot_blob.bin "$BUILD_DIR/" 2>/dev/null || true

# Generate summary
echo ""
echo "=== Build Complete ==="
echo "Output files:"
ls -la "$BUILD_DIR/"
echo ""
echo "To use in FabricOS:"
echo "  1. libv8.a will be linked by kernel/build.rs"
echo "  2. snapshot_blob.bin should be embedded in initramfs"
echo ""
