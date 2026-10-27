//! Goal tests for the `media_type` seam (#10, spec §5.1): detection from the freedesktop
//! shared-mime-info database, in process.
//!
//! Assumed API: `hali::media_type::detect(file_name: &str, head: &[u8]) -> String`, the
//! lowercase `type/subtype`. `file_name` is the final path component; `head` is the first
//! bytes of the content (at most 8 KiB). Order: filename glob, then content magic; an empty
//! `head` uses the glob alone; no match gives `text/plain` for text-looking content (no NUL
//! byte, valid UTF-8 with a cut-off final sequence allowed, empty counts as text) and
//! `application/octet-stream` otherwise.
//!
//! Every test needs the MIME database, so every one is `#[ignore]`d with a reason and runs
//! under `make test-external`.

mod goal {
    use hali::media_type::detect;

    #[test]
    #[ignore = "needs shared-mime-info"]
    fn glob_md() {
        assert_eq!(detect("notes.md", b"# A heading\n"), "text/markdown");
    }

    #[test]
    #[ignore = "needs shared-mime-info"]
    fn glob_beats_magic() {
        // PDF magic, but the name says markdown: the glob wins.
        assert_eq!(
            detect("fake.md", b"%PDF-1.7\n%\xe2\xe3\xcf\xd3\n"),
            "text/markdown"
        );
    }

    #[test]
    #[ignore = "needs shared-mime-info"]
    fn magic_pdf_no_ext() {
        assert_eq!(
            detect("paper", b"%PDF-1.7\n%\xe2\xe3\xcf\xd3\n"),
            "application/pdf"
        );
    }

    #[test]
    #[ignore = "needs shared-mime-info"]
    fn empty_md_is_markdown() {
        // Not application/x-zerosize: an empty file uses its glob.
        assert_eq!(detect("empty.md", b""), "text/markdown");
    }

    #[test]
    #[ignore = "needs shared-mime-info"]
    fn no_match_text_is_plain() {
        assert_eq!(detect("plainfile", b"just some words\n"), "text/plain");
        assert_eq!(
            detect("plainfile", "caf\u{e9} \u{65e5}\u{672c}\n".as_bytes()),
            "text/plain"
        );
    }

    #[test]
    #[ignore = "needs shared-mime-info"]
    fn no_match_binary_is_octet_stream() {
        assert_eq!(
            detect("blob", &[0x00, 0x9f, 0x92, 0x96, 0xde, 0xad, 0xbe, 0xef]),
            "application/octet-stream"
        );
    }

    #[test]
    #[ignore = "needs shared-mime-info"]
    fn empty_without_a_glob_counts_as_text() {
        assert_eq!(detect("plainfile", b""), "text/plain");
    }

    #[test]
    #[ignore = "needs shared-mime-info"]
    fn cut_off_final_utf8_sequence_is_still_text() {
        // "café" cut after the first byte of the two-byte é.
        assert_eq!(detect("plainfile", b"caf\xc3"), "text/plain");
    }

    #[test]
    #[ignore = "needs shared-mime-info"]
    fn nul_byte_or_invalid_utf8_is_binary() {
        assert_eq!(
            detect("plainfile", b"text then \x00 a NUL"),
            "application/octet-stream"
        );
        assert_eq!(
            detect("plainfile", b"bad \xff\xfe bytes in the middle"),
            "application/octet-stream"
        );
    }

    #[test]
    #[ignore = "needs shared-mime-info"]
    fn x_types_come_back_as_x_types() {
        assert_eq!(
            detect("run.sh", b"#!/bin/sh\necho hi\n"),
            "application/x-shellscript"
        );
    }
}
