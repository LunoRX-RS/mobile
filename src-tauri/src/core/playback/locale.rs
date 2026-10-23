//! libmpv refuses to start unless the process-wide numeric locale is "C". GTK (WebKitGTK on Linux)
//! calls `setlocale(LC_ALL, "")` during startup, so a UTF-8 user locale such as en_US.UTF-8 breaks
//! it. Call this on the thread that creates the mpv handle, right before `Mpv::with_initializer`.

#[cfg(unix)]
pub fn numeric_c() {
    // SAFETY: `setlocale` is given a valid NUL-terminated string. It is not thread safe, but the only
    // category touched is LC_NUMERIC and the app never reads it for its own formatting.
    unsafe {
        libc::setlocale(libc::LC_NUMERIC, c"C".as_ptr());
    }
}

#[cfg(not(unix))]
pub fn numeric_c() {}
