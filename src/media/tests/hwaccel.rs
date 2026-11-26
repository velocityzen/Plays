use super::*;

#[test]
fn keeps_ffmpegs_pick_where_it_decodes_on_the_gpu_itself() {
    ffmpeg::init().unwrap();
    for id in [codec::Id::H264, codec::Id::HEVC, codec::Id::VP9] {
        let default = ffmpeg::decoder::find(id).unwrap();
        assert!(instead_of(&default).is_none(), "{}", default.name());
    }
    // Nor is there another to take for one with no hardware anywhere.
    let flac = ffmpeg::decoder::find(codec::Id::FLAC).unwrap();
    assert!(instead_of(&flac).is_none());
}

#[test]
fn takes_ffmpegs_own_av1_decoder_where_the_gpu_decodes_av1() {
    ffmpeg::init().unwrap();
    let names = |id| -> Vec<_> { decoders(id).map(|codec| codec.name().to_owned()).collect() };
    assert_eq!(names(codec::Id::AV1), ["libdav1d", "av1"]);
    // Not its encoders.
    assert_eq!(names(codec::Id::HEVC), ["hevc"]);
    let default = ffmpeg::decoder::find(codec::Id::AV1).unwrap();
    assert_eq!(default.name(), "libdav1d");
    let gpu = KINDS
        .iter()
        .any(|&kind| decodes(codec::Id::AV1, kind) && device(kind).is_some());
    let instead = instead_of(&default);
    assert_eq!(
        instead.map(|codec| codec.name().to_owned()).as_deref(),
        gpu.then_some("av1")
    );
    if let Some(codec) = instead {
        assert!(codec.is_decoder() && codec.id() == codec::Id::AV1);
    }
}

#[cfg(target_os = "macos")]
#[test]
fn asks_videotoolbox_only_of_av1() {
    let videotoolbox = Kind::AV_HWDEVICE_TYPE_VIDEOTOOLBOX;
    assert!(decodes(codec::Id::HEVC, videotoolbox));
    assert!(decodes(codec::Id::AV1, Kind::AV_HWDEVICE_TYPE_VAAPI));
    // Apple chips from the M3 on.
    let brand = std::process::Command::new("sysctl")
        .args(["-n", "machdep.cpu.brand_string"])
        .output()
        .map(|out| String::from_utf8_lossy(&out.stdout).into_owned())
        .unwrap_or_default();
    let m = brand
        .strip_prefix("Apple M")
        .and_then(|rest| rest.split(|c: char| !c.is_ascii_digit()).next())
        .and_then(|generation| generation.parse::<u32>().ok());
    if let Some(generation) = m {
        assert_eq!(
            decodes(codec::Id::AV1, videotoolbox),
            generation >= 3,
            "{brand}"
        );
    }
}
