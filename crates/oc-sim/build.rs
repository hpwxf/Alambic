//! Sets `OC_BUILD_ID`, the build identifier shown under the version on the
//! splash screen (journal/D-025).

fn main() {
    oc_buildinfo::emit();
}
