//! Optional, **experimental** importer for a legally owned copy of KOEI's 1995 *Sangokushi
//! Eiketsuden* (삼국지 영걸전 / 三國志英傑伝), in the spirit of OpenRCT2 reading RCT2 data.
//!
//! Eiketsuden Reloaded runs entirely on its own license-clean base pack; this crate only lets a
//! player convert files from *their own* install into neutral formats (PNG, UTF-8 JSON) in a local
//! folder of their choice. Everything is read-only on the install, nothing is uploaded, no copy
//! protection is circumvented, and the repository and CI never contain original game bytes: every
//! test uses synthetic fixtures produced by this crate's own encoders.
//!
//! The implementation is clean-room: it follows only the written format facts collected in the
//! project's research notes (see `docs/ORIGINAL_DATA.md`). Parts of a format that the notes do not
//! pin down are reported as *unsupported* instead of being guessed.
//!
//! | module | phase | content |
//! |---|---|---|
//! | [`probe`] | P0 | shareable manifest of a folder: names, sizes, SHA-256, header bytes, container summaries |
//! | [`edition`] | P0 | which release a folder holds (Steam 2017, Korean DOS/V, Traditional-Chinese DOS, PC-98 images) |
//! | [`diskimage`] | P0 | PC-98 disk image header detection (D88, Anex86 FDI/HDI) |
//! | [`ls11`] | P1 | `LS11` archives (`.R3`): directory, dictionary + bit-stream codec, and an encoder |
//! | [`table6`] | P1 | 6-byte-table containers (`FACEDAT.R3`, `PACKGRP.R3`) |
//! | [`text`] | P2 | message files (`SNRnM.R3`, `IPPAN0M.R3`) and EUC-KR / Big5 decoding |
//! | [`scenario`] | P2 | scenario bytecode (`SNRnD.R3`): scenes, trigger records, the event instruction set |
//! | [`ippan`] | P2 | townspeople chatter (`IPPAN0.R3` index, `IPPAN0M.R3` strings) |
//! | [`bakdata`] | P2 | `BAKDATA.R3`: townspeople, items and officers (names, stats, initial state) |
//! | [`planar`], [`palette`], [`image`] | P3 | 4 bpp planar cells and packed images, palettes inside `MAIN.EXE`, indexed PNG output |
//! | [`sprites`] | P3 | per-archive geometry, palette slot and entry groups of the sprite / chip archives |
//! | [`extract`] | P1–P3 | conversion into a media overlay folder with an `index.json` |
//! | [`battles`] | original mode | the original battles (setup, rosters, treasures) re-staged onto the base pack's battles |
//! | [`pack`] | original mode | conversion into a layered data pack on top of the base pack (portraits, unit sheets, a tileset learned from the battle maps) |
//! | [`remake`] | original mode | the new art (D27): battle maps redrawn from their terrain grids, this project's unit sheets |
//!
//! Every structural invariant (directory chains, exact decoded lengths, full input consumption,
//! table sizes) is checked, and a violation is reported with a precise error rather than
//! producing partial output.

pub mod bakdata;
pub mod battles;
pub mod chapters;
pub mod diskimage;
pub mod edition;
pub mod extract;
mod flow;
pub mod image;
pub mod install;
pub mod ippan;
pub mod ls11;
pub mod maps;
pub mod music;
pub mod opl;
pub mod pack;
pub mod palette;
pub mod planar;
pub mod probe;
pub mod remake;
pub mod scenario;
pub mod sprites;
pub mod table6;
#[cfg(test)]
mod testutil;
pub mod text;
pub mod tfdce;

/// Lower-case hexadecimal rendering of bytes (hashes, header bytes).
pub fn hex(bytes: &[u8]) -> String {
    use std::fmt::Write as _;
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        let _ = write!(s, "{b:02x}");
    }
    s
}

/// SHA-256 of a byte slice as lower-case hex.
pub fn sha256_hex(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    hex(&Sha256::digest(bytes))
}

/// Version string recorded in manifests and indexes.
pub fn tool_version() -> String {
    format!("hero-import {}", env!("CARGO_PKG_VERSION"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hex_and_sha256() {
        assert_eq!(hex(&[0x00, 0x0f, 0xa5, 0xff]), "000fa5ff");
        // SHA-256 of the empty string (FIPS 180-2 test vector).
        assert_eq!(
            sha256_hex(b""),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
        assert_eq!(
            sha256_hex(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }
}
