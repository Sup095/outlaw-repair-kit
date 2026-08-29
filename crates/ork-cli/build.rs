//! Gives the Windows build an icon and version information.
//!
//! Without this the program is a generic console executable: no icon in a
//! folder, no icon on a shortcut, and nothing in its Properties saying what
//! it is or who made it. That is what "Windows does not see it as an app"
//! looks like from the outside, and it is one resource file away from being
//! fixed.
//!
//! Deliberately unable to fail the build. Embedding a resource needs a
//! resource compiler from the Windows SDK, and a machine without one should
//! still be able to build a working program -- it simply gets a plain icon.
//! An installer that cannot be built at all is a far worse outcome than one
//! that looks slightly less finished.

fn main() {
    println!("cargo:rerun-if-changed=assets/outlaw.ico");

    #[cfg(windows)]
    {
        let mut resource = winresource::WindowsResource::new();
        resource.set_icon("assets/outlaw.ico");
        resource.set("ProductName", "Outlaw Repair Kit");
        resource.set("FileDescription", "Outlaw Repair Kit");
        resource.set("CompanyName", "Outlaw Systems");
        resource.set("LegalCopyright", "Copyright (c) 2026 Outlaw Systems");
        if let Err(error) = resource.compile() {
            println!("cargo:warning=could not embed the icon: {error}");
        }
    }
}
