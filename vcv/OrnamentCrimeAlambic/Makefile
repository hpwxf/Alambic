# If RACK_DIR is not defined when calling the Makefile, default to two
# directories above, matching the layout `helper.py createplugin` assumes.
RACK_DIR ?= ../..

# `plugin.mk` normally defines ARCH_LIN/ARCH_MAC/ARCH_WIN by including this
# itself, but only partway through; the Linux-only LDFLAGS below need it
# earlier, so include it explicitly first. Re-including it from `plugin.mk`
# afterwards is harmless: it only inspects the compiler's target triple.
include $(RACK_DIR)/arch.mk

# The Rack SDK pins `-mmacosx-version-min=10.9`, but the libc++ shipped with
# current macOS SDKs only supports 11.0 and later and warns "The selected
# platform is no longer supported by libc++" on every translation unit. 11.0
# is also the floor of every Apple Silicon Mac and the default deployment
# target of the Rust staticlib linked below. The SDK appends `EXTRA_*` after
# its own flags, and the last `-mmacosx-version-min` wins.
ifdef ARCH_MAC
EXTRA_FLAGS += -mmacosx-version-min=11.0
EXTRA_LDFLAGS += -mmacosx-version-min=11.0
endif

# FLAGS will be passed to both the C and C++ compiler.
FLAGS +=
CFLAGS +=
CXXFLAGS +=

# `oc-vcv-ffi` is a Rust staticlib built by `cargo build --release -p
# oc-vcv-ffi` (equivalently, `cargo xtask vcv build` runs that first). It is
# linked statically, per the Rack SDK's own guidance against relying on the
# user's shared library search path. `xtask` always overrides this path
# explicitly with the workspace's actual target directory; the default here
# only serves a manual `make` invocation from a checkout laid out as this
# repository normally is.
#
# The staticlib is a *prerequisite* of the plugin binary (see the rule after
# `include $(RACK_DIR)/plugin.mk`), not just a linker flag: the SDK's link
# rule only depends on the C++ objects, so a change on the Rust side alone
# (behaviour, or the version shown on the splash screen) would otherwise
# leave a stale plugin.dylib. As a prerequisite it also reaches the link
# line through `$^`, after the objects and before `LDFLAGS`'s libraries.
OC_VCV_FFI_LIB ?= ../../target/release/liboc_vcv_ffi.a

# The Rust staticlib links the standard library, which needs the platform's
# threading and dynamic-loading primitives; macOS provides them through
# libSystem, already linked implicitly, but Linux needs them named explicitly.
ifdef ARCH_LIN
LDFLAGS += -lpthread -ldl -lm
endif

# Add .cpp files to the build.
SOURCES += $(wildcard src/*.cpp)

# Add files to the ZIP package when running `make dist`.
# The compiled plugin and "plugin.json" are automatically added.
DISTRIBUTABLES += res
DISTRIBUTABLES += $(wildcard LICENSE*)

# Include the Rack plugin Makefile framework.
include $(RACK_DIR)/plugin.mk

# Relink whenever the Rust staticlib changes (see OC_VCV_FFI_LIB above).
$(TARGET): $(OC_VCV_FFI_LIB)
