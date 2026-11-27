//! Trust (`docs/updates.md` 4): the identity that signed the running Koyo is the only trust
//! root. No other key exists. The system's code-signing checks are asked in process, of the
//! framework that makes them, so nothing is parsed from a tool's output.

use std::path::{Path, PathBuf};
use std::ptr::{self, NonNull};

use objc2_core_foundation::{CFRetained, CFString, CFURL, Type};
use objc2_security::{
    SecCSFlags, SecCode, SecCopyErrorMessageString, SecRequirement, SecStaticCode,
    kSecCSCheckNestedCode, kSecCSStrictValidate,
};

use super::stage::Failure;
use crate::blocking::Blocking;

/// This process's designated requirement, as text, so it can cross to the job's thread: what
/// any image and any copy must satisfy. A Developer ID build's reads `identifier
/// "codes.koyo.desktop" and anchor apple generic and … certificate leaf[subject.OU] = TEAM`; a
/// build signed here names its certificate's hash. Compiled again where it's checked.
#[derive(Clone, Debug)]
pub(super) struct Requirement(String);

/// A disk image that satisfied the requirement. Copying the app out takes only this, so an
/// unchecked image is never mounted.
#[derive(Debug)]
pub(super) struct Vouched(PathBuf);

impl Vouched {
    pub(super) fn image(&self) -> &Path {
        &self.0
    }
}

impl Requirement {
    /// The requirement the running process's own signature designates: none for one that
    /// isn't signed, with the system's words for why.
    pub(super) fn of_self(_: Blocking<'_>) -> Result<Requirement, String> {
        let flags = SecCSFlags::DefaultFlags;
        // SAFETY: `out` is a valid place for the reference the call writes.
        let running = copied::<SecCode>(|out| unsafe { SecCode::copy_self(flags, out.cast()) })?;
        // SAFETY: `running` is this process's code, and `out` a valid place for the reference.
        let on_disk = copied::<SecStaticCode>(|out| unsafe { running.copy_static_code(flags, out) })?;
        let designated = copied::<SecRequirement>(|out| {
            // SAFETY: `on_disk` is the code just made, and `out` a valid place for the reference.
            unsafe { SecCode::copy_designated_requirement(&on_disk, flags, out.cast()) }
        })?;
        // SAFETY: `designated` is the requirement just made, and `out` a valid place for the text.
        let text = copied::<CFString>(|out| unsafe { designated.copy_string(flags, out) })?;
        Ok(Requirement(text.to_string()))
    }

    /// A requirement from its text, for tests that sign ad hoc: `identifier "codes.koyo.desktop"`.
    #[cfg(test)]
    pub(super) fn text(text: &str) -> Requirement {
        Requirement(text.to_owned())
    }

    /// Checks the downloaded `image` before anything reads what's in it: its signature
    /// satisfies this requirement, and it is whole. The only way to get a [`Vouched`].
    pub(super) fn vouch(&self, image: PathBuf, may: Blocking<'_>) -> Result<Vouched, Failure> {
        self.check(Code::Image(&image), may).map_err(Failure::NotKoyos)?;
        Ok(Vouched(image))
    }

    /// Checks that the bundle at `bundle`, a copy taken out of an image, satisfies this
    /// requirement, and that it and all the code nested in it are whole, so what is put in
    /// place is exactly what Koyo's identity signed.
    pub(super) fn holds_for(&self, bundle: &Path, may: Blocking<'_>) -> Result<(), Failure> {
        self.check(Code::Bundle(bundle), may).map_err(Failure::NotKoyos)
    }

    /// Reads all of `code`, a whole bundle's files among them, to hash it: the system's
    /// words for why it doesn't satisfy this requirement, if it doesn't.
    fn check(&self, code: Code<'_>, _: Blocking<'_>) -> Result<(), String> {
        let flags = SecCSFlags::DefaultFlags;
        let url = CFURL::from_file_path(code.path()).ok_or("its path isn't one the system takes")?;
        let on_disk = copied::<SecStaticCode>(|out| {
            // SAFETY: `url` is a valid URL, and `out` a valid place for the reference.
            unsafe { SecStaticCode::create_with_path(&url, flags, out) }
        })?;
        let text = CFString::from_str(&self.0);
        let requirement = copied::<SecRequirement>(|out| {
            // SAFETY: `text` is a valid string, and `out` a valid place for the reference.
            unsafe { SecRequirement::create_with_string(&text, flags, out.cast()) }
        })?;
        // SAFETY: the code and the requirement are the ones just made.
        let status = unsafe { on_disk.check_validity(code.validated(), Some(&requirement)) };
        if status == 0 { Ok(()) } else { Err(words(status)) }
    }
}

/// What a requirement is checked against, which says how far the check reads.
#[derive(Clone, Copy)]
enum Code<'a> {
    /// A disk image: one file, which its signature seals whole.
    Image(&'a Path),
    /// A bundle. Its seal holds the code nested in it, the engine's framework and helper
    /// apps, only by what each is signed as, so each of those is validated in turn: every
    /// file of every one is hashed, where its signature alone would otherwise be compared.
    Bundle(&'a Path),
}

impl<'a> Code<'a> {
    fn path(self) -> &'a Path {
        match self {
            Code::Image(path) | Code::Bundle(path) => path,
        }
    }

    /// How it's validated: strictly, and a bundle's nested code with it.
    fn validated(self) -> SecCSFlags {
        match self {
            Code::Image(_) => SecCSFlags(kSecCSStrictValidate),
            Code::Bundle(_) => SecCSFlags(kSecCSStrictValidate | kSecCSCheckNestedCode),
        }
    }
}

/// Runs `call`, a Security call that writes an object for its caller to own, and takes the
/// object: failed, the system's words for why.
fn copied<T: Type>(call: impl FnOnce(NonNull<*const T>) -> i32) -> Result<CFRetained<T>, String> {
    let mut made: *const T = ptr::null();
    let status = call(NonNull::from(&mut made));
    match NonNull::new(made.cast_mut()) {
        // SAFETY: the call succeeded and wrote an object that, made by a Copy or a Create
        // call, is its caller's to release.
        Some(made) if status == 0 => Ok(unsafe { CFRetained::from_raw(made) }),
        _ => Err(words(status)),
    }
}

/// What the system says of `status`, a Security call's failure.
fn words(status: i32) -> String {
    // SAFETY: the reserved pointer is null, as the call asks.
    let said = unsafe { SecCopyErrorMessageString(status, ptr::null_mut()) };
    said.map_or_else(|| format!("the system's error {status}"), |said| said.to_string())
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::Requirement;
    use crate::blocking::in_test;
    use crate::test_support::{scratch, write};
    use crate::updates::fixtures::{self, Made};
    use crate::updates::stage::Failure;

    #[test]
    fn a_bundle_whose_nested_code_was_changed_after_it_was_signed_is_refused() {
        let bundle = scratch("update-nested").join("Koyo.app");
        let helper = fixtures::bundle_with_helper(&bundle, &Made::release("9.0.0"));
        let trust = fixtures::trust();
        assert_eq!(in_test(|may| trust.holds_for(&bundle, may)), Ok(()));

        write(&helper, "Contents/Resources/words.txt", b"changed");

        let changed = in_test(|may| trust.holds_for(&bundle, may));
        assert!(matches!(changed, Err(Failure::NotKoyos(_))), "{changed:?}");
    }

    #[test]
    fn this_process_s_own_requirement_holds_for_its_executable_and_not_for_another() {
        let trust = in_test(Requirement::of_self).unwrap();
        let exe = std::env::current_exe().unwrap();
        assert_eq!(in_test(|may| trust.holds_for(&exe, may)), Ok(()));
        let other = in_test(|may| trust.holds_for(Path::new("/usr/bin/true"), may));
        assert!(matches!(other, Err(Failure::NotKoyos(_))), "{other:?}");
        let missing = in_test(|may| trust.holds_for(Path::new("/no/such/Koyo.app"), may));
        assert!(matches!(missing, Err(Failure::NotKoyos(_))), "{missing:?}");
    }
}
