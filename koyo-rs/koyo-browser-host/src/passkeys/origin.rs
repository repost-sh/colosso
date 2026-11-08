//! Whether a frame may use passkeys, and for which site: its origin, from the address the
//! engine has for it, must be a secure context and the same as every frame's above it; and
//! the relying party's id must be the origin's domain or a registrable suffix of it, never
//! a public suffix (`co.uk`, `github.io`), as HTML's "is a registrable domain suffix of or
//! is equal to" and `WebAuthn` say.
use koyo_browser_ipc::{PasskeyError, Refusal};
use url::{Host, Origin, Url};

/// The origin a frame at `address` has, when it isn't opaque: `blob:` addresses have the
/// origin inside them; `about:blank`, `data:` and the like have none of their own.
fn origin_of(address: &str) -> Option<Origin> {
    let origin = Url::parse(address).ok()?.origin();
    origin.is_tuple().then_some(origin)
}

/// Whether `origin` is a secure context: `https`, or `http` on this machine by name
/// (`localhost`, `*.localhost`) or by loopback address.
fn secure(origin: &Origin) -> bool {
    let Origin::Tuple(scheme, host, _) = origin else {
        return false;
    };
    match scheme.as_str() {
        "https" => true,
        "http" => match host {
            Host::Domain(domain) => domain == "localhost" || domain.ends_with(".localhost"),
            Host::Ipv4(address) => address.is_loopback(),
            Host::Ipv6(address) => address.is_loopback(),
        },
        _ => false,
    }
}

/// What a frame's request may go ahead as: its serialised origin and the relying party's id.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Allowed {
    /// `https://accounts.example.com`.
    pub(crate) origin: String,
    /// The id the request is made for: the one the page named, or else the origin's domain.
    pub(crate) rp_id: String,
}

/// Checks a request from the frame at `address`, under frames at `ancestors` (its parent
/// first), for the relying party the page named (`rp_id`, empty for none).
///
/// # Errors
/// The refusal the page gets: `NotAllowedError` for an origin of no use (opaque, not
/// secure, under another origin's frame), `SecurityError` for an id that doesn't fit.
pub(crate) fn check(address: &str, ancestors: &[String], rp_id: &str) -> Result<Allowed, Refusal> {
    let not_allowed = |message: &str| Refusal::new(PasskeyError::NotAllowed, message);
    let Some(origin) = origin_of(address).filter(secure) else {
        return Err(not_allowed(
            "Passkeys are only for secure pages (https, or http on localhost).",
        ));
    };
    if ancestors
        .iter()
        .any(|above| origin_of(above).as_ref() != Some(&origin))
    {
        return Err(not_allowed(
            "The 'publickey-credentials-get' feature is not enabled in this document. \
             Permissions Policy may be used to delegate Web Authentication capabilities to \
             cross-origin child frames.",
        ));
    }
    let security = |message: &str| Refusal::new(PasskeyError::Security, message);
    let Origin::Tuple(_, Host::Domain(domain), _) = &origin else {
        return Err(security("This is an invalid domain."));
    };
    let rp_id = if rp_id.is_empty() {
        domain.clone()
    } else {
        match Host::parse(rp_id) {
            Ok(Host::Domain(named)) if suffix_of_or_equal(&named, domain) => named,
            _ => {
                return Err(security(
                    "The relying party ID is not a registrable domain suffix of, nor equal to \
                     the current domain.",
                ));
            }
        }
    };
    Ok(Allowed {
        origin: origin.ascii_serialization(),
        rp_id,
    })
}

/// HTML's "is a registrable domain suffix of or is equal to", for two domains in their
/// ASCII form: `suffix` is `host`, or a suffix of it at a label's edge that isn't a public
/// suffix and doesn't reach into `host`'s own public suffix.
pub(crate) fn suffix_of_or_equal(suffix: &str, host: &str) -> bool {
    if suffix.is_empty() {
        return false;
    }
    if suffix == host {
        return true;
    }
    let dotted = format!(".{suffix}");
    if !host.ends_with(&dotted) {
        return false;
    }
    let public = |domain: &str| psl::suffix_str(domain).unwrap_or(domain).to_owned();
    // A public suffix itself, or a part of the host's: shared by sites that aren't one.
    suffix != public(suffix) && !public(host).ends_with(&dotted)
}

#[cfg(test)]
mod tests;
