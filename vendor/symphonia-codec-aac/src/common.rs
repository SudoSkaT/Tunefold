// Symphonia
// Copyright (c) 2019-2022 The Project Symphonia Developers.
//
// Previous Author: Kostya Shishkov <kostya.shiskov@gmail.com>
//
// This source file includes code originally written for the NihAV
// project. With the author's permission, it has been relicensed for,
// and ported to the Symphonia project.
//
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use symphonia_core::audio::Channels;
use symphonia_core::errors::{decode_error, Result};
use symphonia_core::io::ReadBitsLtr;

#[allow(non_camel_case_types)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum M4AType {
    None,
    Main,
    Lc,
    Ssr,
    Ltp,
    Sbr,
    Scalable,
    TwinVQ,
    Celp,
    Hvxc,
    Ttsi,
    MainSynth,
    WavetableSynth,
    GeneralMIDI,
    Algorithmic,
    ER_AAC_LC,
    ER_AAC_LTP,
    ER_AAC_Scalable,
    ER_TwinVQ,
    ER_BSAC,
    ER_AAC_LD,
    ER_CELP,
    ER_HVXC,
    ER_HILN,
    ER_Parametric,
    Ssc,
    PS,
    MPEGSurround,
    Layer1,
    Layer2,
    Layer3,
    Dst,
    Als,
    Sls,
    SLSNonCore,
    ER_AAC_ELD,
    SMRSimple,
    SMRMain,
    Reserved,
    Unknown,
}

impl std::fmt::Display for M4AType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", M4A_TYPE_NAMES[*self as usize])
    }
}

pub const M4A_TYPES: &[M4AType] = &[
    M4AType::None,
    M4AType::Main,
    M4AType::Lc,
    M4AType::Ssr,
    M4AType::Ltp,
    M4AType::Sbr,
    M4AType::Scalable,
    M4AType::TwinVQ,
    M4AType::Celp,
    M4AType::Hvxc,
    M4AType::Reserved,
    M4AType::Reserved,
    M4AType::Ttsi,
    M4AType::MainSynth,
    M4AType::WavetableSynth,
    M4AType::GeneralMIDI,
    M4AType::Algorithmic,
    M4AType::ER_AAC_LC,
    M4AType::Reserved,
    M4AType::ER_AAC_LTP,
    M4AType::ER_AAC_Scalable,
    M4AType::ER_TwinVQ,
    M4AType::ER_BSAC,
    M4AType::ER_AAC_LD,
    M4AType::ER_CELP,
    M4AType::ER_HVXC,
    M4AType::ER_HILN,
    M4AType::ER_Parametric,
    M4AType::Ssc,
    M4AType::PS,
    M4AType::MPEGSurround,
    M4AType::Reserved, /*escape*/
    M4AType::Layer1,
    M4AType::Layer2,
    M4AType::Layer3,
    M4AType::Dst,
    M4AType::Als,
    M4AType::Sls,
    M4AType::SLSNonCore,
    M4AType::ER_AAC_ELD,
    M4AType::SMRSimple,
    M4AType::SMRMain,
];

pub const M4A_TYPE_NAMES: &[&str] = &[
    "None",
    "AAC Main",
    "AAC LC",
    "AAC SSR",
    "AAC LTP",
    "SBR",
    "AAC Scalable",
    "TwinVQ",
    "CELP",
    "HVXC",
    // "(reserved10)",
    // "(reserved11)",
    "TTSI",
    "Main synthetic",
    "Wavetable synthesis",
    "General MIDI",
    "Algorithmic Synthesis and Audio FX",
    "ER AAC LC",
    // "(reserved18)",
    "ER AAC LTP",
    "ER AAC Scalable",
    "ER TwinVQ",
    "ER BSAC",
    "ER AAC LD",
    "ER CELP",
    "ER HVXC",
    "ER HILN",
    "ER Parametric",
    "SSC",
    "PS",
    "MPEG Surround",
    // "(escape)",
    "Layer-1",
    "Layer-2",
    "Layer-3",
    "DST",
    "ALS",
    "SLS",
    "SLS non-core",
    "ER AAC ELD",
    "SMR Simple",
    "SMR Main",
    "(reserved)",
    "(unknown)",
];

pub const AAC_SAMPLE_RATES: [u32; 16] = [
    96000, 88200, 64000, 48000, 44100, 32000, 24000, 22050, 16000, 12000, 11025, 8000, 7350, 0, 0,
    0,
];

pub const AAC_CHANNELS: [usize; 8] = [0, 1, 2, 3, 4, 5, 6, 8];

/// Program Config Element (PCE).
///
/// A PCE describes the channel layout of an AAC stream when the Audio Specific
/// Config (or the ADTS header) uses `channel_configuration = 0`. It is a
/// variable-length syntax element, so it must be consumed exactly even when
/// the decoder already knows the layout from the container.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ProgramConfig {
    pub object_type: M4AType,
    pub sample_rate: u32,
    pub channels: usize,
}

/// Reads a `program_config_element()` and consumes its comment field.
///
/// This implements the syntax from ISO/IEC 13818-7 / 14496-3. The element
/// instance tag is intentionally validated only for sanity; the caller is
/// responsible for deciding whether the profile is supported.
pub fn read_program_config_element<B: ReadBitsLtr>(bs: &mut B) -> Result<ProgramConfig> {
    let _element_instance_tag = bs.read_bits_leq32(4)?;

    let object_type = match bs.read_bits_leq32(2)? {
        idx @ 0..=2 => M4A_TYPES[(idx + 1) as usize],
        _ => M4AType::Reserved,
    };

    let sample_rate = match bs.read_bits_leq32(4)? {
        idx if idx < 13 => AAC_SAMPLE_RATES[idx as usize],
        15 => bs.read_bits_leq32(24)?,
        _ => return decode_error("aac: pce reserved sample rate"),
    };

    let num_front = bs.read_bits_leq32(4)? as usize;
    let num_side = bs.read_bits_leq32(4)? as usize;
    let num_back = bs.read_bits_leq32(4)? as usize;
    let num_lfe = bs.read_bits_leq32(2)? as usize;
    let num_assoc_data = bs.read_bits_leq32(3)? as usize;
    let num_valid_cc = bs.read_bits_leq32(4)? as usize;

    if bs.read_bool()? {
        let _mono_mixdown = bs.read_bits_leq32(4)?;
    }
    if bs.read_bool()? {
        let _stereo_mixdown = bs.read_bits_leq32(4)?;
    }
    if bs.read_bool()? {
        let _matrix_mixdown_idx = bs.read_bits_leq32(2)?;
        let _pseudo_surround_enable = bs.read_bool()?;
    }

    let mut channels = 0usize;

    for _ in 0..num_front {
        let is_pair = bs.read_bool()?;
        let _tag = bs.read_bits_leq32(4)?;
        channels += if is_pair { 2 } else { 1 };
    }
    for _ in 0..num_side {
        let is_pair = bs.read_bool()?;
        let _tag = bs.read_bits_leq32(4)?;
        channels += if is_pair { 2 } else { 1 };
    }
    for _ in 0..num_back {
        let is_pair = bs.read_bool()?;
        let _tag = bs.read_bits_leq32(4)?;
        channels += if is_pair { 2 } else { 1 };
    }
    for _ in 0..num_lfe {
        let _tag = bs.read_bits_leq32(4)?;
        channels += 1;
    }
    for _ in 0..num_assoc_data {
        let _tag = bs.read_bits_leq32(4)?;
    }
    for _ in 0..num_valid_cc {
        let _is_ind_sw = bs.read_bool()?;
        let _tag = bs.read_bits_leq32(4)?;
    }

    bs.realign();

    let comment_bytes = bs.read_bits_leq32(8)?;
    bs.ignore_bits(comment_bytes * 8)?;

    if channels == 0 || sample_rate == 0 {
        return decode_error("aac: invalid program config element");
    }

    Ok(ProgramConfig { object_type, sample_rate, channels })
}

pub fn map_channels(channels: u32) -> Option<Channels> {
    match channels {
        0 => None,
        1 => Some(Channels::FRONT_LEFT),
        2 => Some(Channels::FRONT_LEFT | Channels::FRONT_RIGHT),
        3 => Some(Channels::FRONT_CENTRE | Channels::FRONT_LEFT | Channels::FRONT_RIGHT),
        4 => Some(
            Channels::FRONT_CENTRE
                | Channels::FRONT_LEFT
                | Channels::FRONT_RIGHT
                | Channels::REAR_CENTRE,
        ),
        5 => Some(
            Channels::FRONT_CENTRE
                | Channels::FRONT_LEFT
                | Channels::FRONT_RIGHT
                | Channels::SIDE_LEFT
                | Channels::SIDE_RIGHT,
        ),
        6 => Some(
            Channels::FRONT_CENTRE
                | Channels::FRONT_LEFT
                | Channels::FRONT_RIGHT
                | Channels::SIDE_LEFT
                | Channels::SIDE_RIGHT
                | Channels::LFE1,
        ),
        7 => None,
        8 => Some(
            Channels::FRONT_CENTRE
                | Channels::FRONT_LEFT
                | Channels::FRONT_RIGHT
                | Channels::SIDE_LEFT
                | Channels::SIDE_RIGHT
                | Channels::FRONT_LEFT_WIDE
                | Channels::FRONT_RIGHT_WIDE
                | Channels::LFE1,
        ),
        _ => None,
    }
}
