// SPDX-License-Identifier: MIT

//! Name-component checks: template components (strict) and listed names (lenient), reserved
//! device names and the 8.3 short-name detector.
//!
//! Ownership: work package **B**.

use super::PathReject;

/// Longest component in UTF-16 units (NTFS `MAX_COMPONENT_LENGTH`).
pub(crate) const MAX_COMPONENT: usize = 255;

const DOT: u16 = b'.' as u16;
const SPACE: u16 = b' ' as u16;
const TILDE: u16 = b'~' as u16;

/// Strict check for a component that comes from a root template (M1 design section 4.1 step 5):
/// rejects empty, `.`, `..`, `:`, `<>"|`, control chars, `*`/`?`, trailing dot or space, `~`
/// followed by an ASCII digit (8.3), reserved device names and components over 255 units.
///
/// Also (defence in depth, for direct callers of `WidePath::push_template_component`): NUL
/// ([`PathReject::NulChar`]), `\` and `/` ([`PathReject::InvalidChar`]) and lone surrogates
/// ([`PathReject::LoneSurrogateInTemplate`]).
pub fn check_template_component(c: &[u16]) -> Result<(), PathReject> {
    check_dots(c)?;
    if c.contains(&0) {
        return Err(PathReject::NulChar);
    }
    if c.contains(&u16::from(b':')) {
        return Err(PathReject::AlternateStream);
    }
    if let Some(&bad) = c.iter().find(|&&u| is_invalid_template_char(u)) {
        return Err(PathReject::InvalidChar(bad));
    }
    if c.iter()
        .any(|&u| u == u16::from(b'*') || u == u16::from(b'?'))
    {
        return Err(PathReject::Wildcard);
    }
    if char::decode_utf16(c.iter().copied()).any(|r| r.is_err()) {
        return Err(PathReject::LoneSurrogateInTemplate);
    }
    if matches!(c.last(), Some(&DOT) | Some(&SPACE)) {
        return Err(PathReject::TrailingDotOrSpace);
    }
    if has_short_name_marker(c) {
        return Err(PathReject::ShortName);
    }
    if is_reserved_device_name(c) {
        return Err(PathReject::ReservedDeviceName);
    }
    if c.len() > MAX_COMPONENT {
        return Err(PathReject::ComponentTooLong);
    }
    Ok(())
}

/// Lenient check for a name returned by a directory listing: rejects only empty, `.`, `..`,
/// `\`, `/`, `:`, NUL and names over 255 units. Listed names are never re-parsed.
pub fn check_listed_name(c: &[u16]) -> Result<(), PathReject> {
    check_dots(c)?;
    if c.contains(&0) {
        return Err(PathReject::NulChar);
    }
    if let Some(&bad) = c
        .iter()
        .find(|&&u| u == u16::from(b'\\') || u == u16::from(b'/'))
    {
        return Err(PathReject::InvalidChar(bad));
    }
    if c.contains(&u16::from(b':')) {
        return Err(PathReject::AlternateStream);
    }
    if c.len() > MAX_COMPONENT {
        return Err(PathReject::ComponentTooLong);
    }
    Ok(())
}

/// [`check_listed_name`] plus the characters no Windows file name can contain (`<>"|*?` and
/// control characters). Used for known-folder API values, which are lenient on 8.3 names
/// (design deviation 8) but must still look like a real path.
pub(crate) fn check_known_component(c: &[u16]) -> Result<(), PathReject> {
    check_listed_name(c)?;
    if c.iter()
        .any(|&u| u == u16::from(b'*') || u == u16::from(b'?'))
    {
        return Err(PathReject::Wildcard);
    }
    if let Some(&bad) = c.iter().find(|&&u| is_invalid_template_char(u)) {
        return Err(PathReject::InvalidChar(bad));
    }
    Ok(())
}

fn check_dots(c: &[u16]) -> Result<(), PathReject> {
    match c {
        [] => Err(PathReject::EmptyComponent),
        [DOT] => Err(PathReject::CurrentDirComponent),
        [DOT, DOT] => Err(PathReject::ParentTraversal),
        _ => Ok(()),
    }
}

/// `<>"|`, `\`, `/` and control characters below 0x20 (NUL is reported separately).
fn is_invalid_template_char(u: u16) -> bool {
    u < 0x20
        || u == u16::from(b'<')
        || u == u16::from(b'>')
        || u == u16::from(b'"')
        || u == u16::from(b'|')
        || u == u16::from(b'\\')
        || u == u16::from(b'/')
}

/// `~` immediately followed by an ASCII digit anywhere in the component (`PROGRA~1`,
/// `a~1.tmp`).
fn has_short_name_marker(c: &[u16]) -> bool {
    c.windows(2)
        .any(|w| matches!(w, [TILDE, d] if (u16::from(b'0')..=u16::from(b'9')).contains(d)))
}

/// The stem (before the first `.`, trailing spaces trimmed, ASCII case-insensitive) is one of
/// `CON PRN AUX NUL COM0-9 LPT0-9 COM¹²³ LPT¹²³ CONIN$ CONOUT$`.
fn is_reserved_device_name(c: &[u16]) -> bool {
    let stem_end = c.iter().position(|&u| u == DOT).unwrap_or(c.len());
    let mut stem = c.get(..stem_end).unwrap_or(&[]);
    while let [rest @ .., SPACE] = stem {
        stem = rest;
    }
    let up: Vec<u16> = stem
        .iter()
        .map(|&u| {
            if (u16::from(b'a')..=u16::from(b'z')).contains(&u) {
                u - 32
            } else {
                u
            }
        })
        .collect();
    let is = |s: &str| up.iter().copied().eq(s.encode_utf16());
    if ["CON", "PRN", "AUX", "NUL", "CONIN$", "CONOUT$"]
        .iter()
        .any(|s| is(s))
    {
        return true;
    }
    match up.as_slice() {
        [a, b, c3, n] => {
            let com = [*a, *b, *c3] == [u16::from(b'C'), u16::from(b'O'), u16::from(b'M')];
            let lpt = [*a, *b, *c3] == [u16::from(b'L'), u16::from(b'P'), u16::from(b'T')];
            let digit = (u16::from(b'0')..=u16::from(b'9')).contains(n)
                || matches!(*n, 0x00B9 | 0x00B2 | 0x00B3);
            (com || lpt) && digit
        }
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn w(s: &str) -> Vec<u16> {
        s.encode_utf16().collect()
    }

    #[test]
    fn template_component_table() {
        use PathReject as R;
        let long = "a".repeat(256);
        let max = "a".repeat(255);
        let cases: Vec<(&str, Result<(), PathReject>)> = vec![
            ("", Err(R::EmptyComponent)),
            (".", Err(R::CurrentDirComponent)),
            ("..", Err(R::ParentTraversal)),
            ("...", Err(R::TrailingDotOrSpace)),
            ("a:b", Err(R::AlternateStream)),
            ("file.txt:stream", Err(R::AlternateStream)),
            ("file.txt::$DATA", Err(R::AlternateStream)),
            ("a<b", Err(R::InvalidChar(u16::from(b'<')))),
            ("a>b", Err(R::InvalidChar(u16::from(b'>')))),
            ("a\"b", Err(R::InvalidChar(u16::from(b'"')))),
            ("a|b", Err(R::InvalidChar(u16::from(b'|')))),
            ("a\\b", Err(R::InvalidChar(u16::from(b'\\')))),
            ("a/b", Err(R::InvalidChar(u16::from(b'/')))),
            ("a\tb", Err(R::InvalidChar(9))),
            ("a\u{1f}", Err(R::InvalidChar(0x1f))),
            ("a\0b", Err(R::NulChar)),
            ("*.tmp", Err(R::Wildcard)),
            ("a?", Err(R::Wildcard)),
            ("x.", Err(R::TrailingDotOrSpace)),
            ("x ", Err(R::TrailingDotOrSpace)),
            ("nul.", Err(R::TrailingDotOrSpace)),
            ("PROGRA~1", Err(R::ShortName)),
            ("a~1.tmp", Err(R::ShortName)),
            ("WINDOW~1", Err(R::ShortName)),
            ("a~b", Ok(())),
            ("a~", Ok(())),
            ("CON", Err(R::ReservedDeviceName)),
            ("con", Err(R::ReservedDeviceName)),
            ("aux.txt", Err(R::ReservedDeviceName)),
            ("NUL.tar.gz", Err(R::ReservedDeviceName)),
            ("CON .txt", Err(R::ReservedDeviceName)),
            ("prn", Err(R::ReservedDeviceName)),
            ("COM1", Err(R::ReservedDeviceName)),
            ("com0", Err(R::ReservedDeviceName)),
            ("LPT9", Err(R::ReservedDeviceName)),
            ("COM\u{00B9}", Err(R::ReservedDeviceName)),
            ("lpt\u{00B2}.log", Err(R::ReservedDeviceName)),
            ("LPT\u{00B3}", Err(R::ReservedDeviceName)),
            ("CONIN$", Err(R::ReservedDeviceName)),
            ("conout$.x", Err(R::ReservedDeviceName)),
            ("COM10", Ok(())),
            ("COMA", Ok(())),
            ("LPTX", Ok(())),
            ("CONSOLE", Ok(())),
            ("nul_", Ok(())),
            ("xCON", Ok(())),
            (" CON", Ok(())),
            ("Temp", Ok(())),
            ("İ ß \u{212A}", Ok(())),
            (".hidden", Ok(())),
            ("a b.c", Ok(())),
            (&max, Ok(())),
            (&long, Err(R::ComponentTooLong)),
        ];
        for (input, expected) in cases {
            assert_eq!(check_template_component(&w(input)), expected, "{input:?}");
        }
        assert_eq!(
            check_template_component(&[0xD800, u16::from(b'a')]),
            Err(R::LoneSurrogateInTemplate)
        );
    }

    #[test]
    fn listed_name_table() {
        use PathReject as R;
        let long = "a".repeat(256);
        let cases: Vec<(&str, Result<(), PathReject>)> = vec![
            ("", Err(R::EmptyComponent)),
            (".", Err(R::CurrentDirComponent)),
            ("..", Err(R::ParentTraversal)),
            ("a\\b", Err(R::InvalidChar(u16::from(b'\\')))),
            ("a/b", Err(R::InvalidChar(u16::from(b'/')))),
            ("a:b", Err(R::AlternateStream)),
            ("a\0", Err(R::NulChar)),
            (&long, Err(R::ComponentTooLong)),
            // Listed names are taken literally: all of these are fine.
            ("CON", Ok(())),
            ("x.", Ok(())),
            ("x ", Ok(())),
            ("PROGRA~1", Ok(())),
            ("a*b", Ok(())),
            ("...", Ok(())),
        ];
        for (input, expected) in cases {
            assert_eq!(check_listed_name(&w(input)), expected, "{input:?}");
        }
        assert_eq!(check_listed_name(&[0xD800]), Ok(()));
    }

    #[test]
    fn known_component() {
        assert_eq!(check_known_component(&w("PROGRA~1")), Ok(()));
        assert_eq!(check_known_component(&w("a*")), Err(PathReject::Wildcard));
        assert_eq!(
            check_known_component(&w("a|")),
            Err(PathReject::InvalidChar(u16::from(b'|')))
        );
        assert_eq!(
            check_known_component(&w("..")),
            Err(PathReject::ParentTraversal)
        );
    }
}
