#!/bin/bash
# V8 Cross-Compilation Script for FabricOS
# Target: x86_64-unknown-none (bare metal)
# Uses custom platform: src/v8_platform
# V8 Version: 13.3-lkgr (JIT-less, Lite mode for embedded)

set -e

# Configuration
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(dirname "$SCRIPT_DIR")"
V8_DIR="$PROJECT_ROOT/vendor/v8"
BUILD_DIR="$PROJECT_ROOT/build/v8"
V8_PLATFORM_DIR="$PROJECT_ROOT/kernel/src/v8_platform"

# V8 Version - using LKGR (Last Known Good Revision) for stability
V8_VERSION="13.3-lkgr"

# Target configuration
TARGET_ARCH="x64"
TARGET_OS="fabricos"

echo "=== V8 Build for FabricOS ==="
echo "V8 Version: $V8_VERSION (JIT-less, Lite mode)"
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
        git clone --depth 1 https://chromium.googlesource.com/chromium/tools/depot_tools.git "$DEPOT_TOOLS"
    fi
fi

export PATH="$DEPOT_TOOLS:$PATH"

# Clone V8 if not present
# NOTE: We use 'fetch v8' instead of 'git clone --depth 1' because:
# - gclient sync needs full git history to resolve dependencies
# - V8 build system requires depot_tools metadata
# - Shallow clones break gclient dependency resolution
if [ ! -d "$V8_DIR/v8" ]; then
    echo "Fetching V8 source (this may take several minutes)..."
    mkdir -p "$V8_DIR"
    cd "$V8_DIR"
    
    # Fetch V8 with all dependencies
    fetch v8
    
    cd v8
    
    # Checkout specific LKGR version (Last Known Good Revision)
    echo "Checking out V8 $V8_VERSION..."
    git checkout "$V8_VERSION"
    
    # Sync dependencies to match this version
    echo "Syncing dependencies..."
    gclient sync
    
    echo "V8 source fetch complete"
else
    echo "V8 source already exists at $V8_DIR/v8"
    echo "To re-fetch, delete $V8_DIR and run again"
fi

cd "$V8_DIR/v8"

echo "=== Applying FabricOS patches ==="

# Create FabricOS platform directory
mkdir -p src/base/platform/fabricos

# Copy our platform implementation
echo "Installing FabricOS platform files..."
cp -r "$V8_PLATFORM_DIR"/* src/base/platform/fabricos/ 2>/dev/null || true

echo "=== Generating build configuration ==="

# Generate GN args for FabricOS - JIT-less configuration
cat > out/fabricos/args.gn << 'EOF'
# V8 Build Configuration for FabricOS
# JIT-less, Lite mode for bare metal kernel integration

is_debug = false
is_official_build = true

# Target platform
target_cpu = "x64"

# Build type
is_component_build = false
is_static = true

# Features for bare metal - DISABLED for JIT-less mode
v8_enable_i18n_support = false
v8_enable_gdbjit = false
v8_use_snapshot = true
v8_use_external_startup_data = false

# No standard library
use_custom_libcxx = false
use_sysroot = false

# JIT-less mode (required for kernel/embedded without writable+executable pages)
v8_enable_jit = false
v8_enable_maglev = false
v8_enable_turbofan = false
v8_enable_sparkplug = false

# Lite mode for smaller footprint
v8_enable_lite_mode = true

# Pointer compression settings
v8_enable_pointer_compression = false
v8_enable_pointer_compression_in_isolate_cage = false
v8_enable_pointer_compression_shared_cage = false

# Optimizations
v8_enable_fast_maths = true
v8_enable_future = false  # Disable experimental features

# Logging
v8_enable_logging_and_profiling = false

# Snapshot - use embedded snapshot for kernel
v8_use_snapshot = true
v8_embedded_builtins = true

# Disable features not needed in kernel
v8_enable_webassembly = false
v8_enable_sandbox = false
v8_enable_heap_sandbox = false
v8_enable_third_party_heap = false

# Custom definitions for JIT-less bare metal
extra_cflags = [
    "-DV8_OS_FABRICOS",
    "-DV8_TARGET_OS_FABRICOS",
    "-DV8_JITLESS",
    "-DV8_LITE_MODE",
    "-DV8_COMPRESS_POINTERS_IN_MULTIPLE_CAGES=0",
    "-DV8_31BIT_SMIS_ON_64BIT_ARCH",
    "-nostdlib",
    "-fno-exceptions",
    "-fno-rtti",
    "-ffreestanding",
    "-m64",
    "-march=x86-64",
    "-fno-stack-protector",
    "-fno-unwind-tables",
    "-fno-asynchronous-unwind-tables",
    "-fvisibility=hidden",
]

extra_ldflags = [
    "-nostdlib",
    "-static",
    "-Wl,--gc-sections",
]

# Treat warnings as errors (disable for now during bring-up)
treat_warnings_as_errors = false
EOF

# Create output directory
mkdir -p out/fabricos

# Generate build files
echo "Running gn gen..."
gn gen out/fabricos

echo "=== Building V8 (JIT-less mode) ==="
echo "This may take 30-60 minutes..."
echo "RAM usage: ~8-12GB (JIT-less uses less than JIT mode)"

# Build V8 static libraries
ninja -C out/fabricos v8_monolith

# Collect outputs
echo "=== Collecting build artifacts ==="

mkdir -p "$BUILD_DIR"

# Copy static libraries - try multiple locations
if [ -f "out/fabricos/obj/libv8_monolith.a" ]; then
    cp out/fabricos/obj/libv8_monolith.a "$BUILD_DIR/libv8.a"
elif [ -f "out/fabricos/obj/v8/libv8_snapshot.a" ]; then
    cp out/fabricos/obj/v8/libv8_snapshot.a "$BUILD_DIR/libv8.a"
elif [ -f "out/fabricos/libv8.a" ]; then
    cp out/fabricos/libv8.a "$BUILD_DIR/libv8.a"
else
    echo "WARNING: Could not find libv8.a - searching..."
    find out/fabricos -name "*.a" -type f | head -20
fi

# Copy snapshot blob
if [ -f "out/fabricos/snapshot_blob.bin" ]; then
    cp out/fabricos/snapshot_blob.bin "$BUILD_DIR/"
elif [ -f "out/fabricos/embedded_snapshot_blob.bin" ]; then
    cp out/fabricos/embedded_snapshot_blob.bin "$BUILD_DIR/snapshot_blob.bin"
fi

# Generate build info
cat > "$BUILD_DIR/build_info.txt" << EOF
V8 Version: $V8_VERSION
Build Date: $(date)
Build Mode: JIT-less, Lite mode
Target: x86_64-unknown-none (FabricOS)
EOF

# Generate summary
echo ""
echo "=== Build Complete ==="
echo "Output files:"
ls -la "$BUILD_DIR/"
echo ""
echo "Build info:"
cat "$BUILD_DIR/build_info.txt"
echo ""
echo "To use in FabricOS:"
echo "  1. libv8.a will be linked by kernel/build.rs"
echo "  2. snapshot_blob.bin should be embedded in initramfs"
echo "  3. Link with libopenlibm.a for math functions"
echo ""
