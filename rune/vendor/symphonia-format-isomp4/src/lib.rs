// Symphonia
// Copyright (c) 2019-2022 The Project Symphonia Developers.
//
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

#![warn(rust_2018_idioms)]
// The following lints are allowed in all Symphonia crates. Please see clippy.toml for their
// justification.
#![allow(clippy::comparison_chain)]
#![allow(clippy::excessive_precision)]
#![allow(clippy::identity_op)]
#![allow(clippy::manual_range_contains)]

mod atoms;
mod demuxer;
mod fourcc;
mod fp;
mod stream;

pub use demuxer::IsoMp4Reader;

/// micro-daw patch: read exactly `len` bytes without trusting `len` (a field of the file) for the
/// allocation size — the buffer grows in 64 KiB steps as the bytes are actually read, so a corrupt
/// length fails with an I/O error instead of allocating gigabytes.
pub(crate) fn read_bytes_bounded<B: symphonia_core::io::ReadBytes>(
    reader: &mut B,
    len: usize,
) -> std::io::Result<Box<[u8]>> {
    const CHUNK: usize = 1 << 16;
    let mut out = Vec::with_capacity(len.min(CHUNK));
    while out.len() < len {
        let start = out.len();
        out.resize(start + (len - start).min(CHUNK), 0);
        reader.read_buf_exact(&mut out[start..])?;
    }
    Ok(out.into_boxed_slice())
}

/// micro-daw patch: capacity for a table whose entry count is a field of the file.
#[allow(dead_code)]
pub(crate) fn bounded_capacity(count: u64) -> usize {
    count.min(4096) as usize
}
