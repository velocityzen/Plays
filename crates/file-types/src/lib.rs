//! The video and audio formats Plays opens, and what macOS and Linux are
//! told of them: on macOS the document types Finder offers Plays for and
//! the type IDs it may not know (`macos`), on Linux the MIME types in the
//! desktop file (`linux`). `script/file-types` writes those, and an icon
//! for each format, from `FORMATS`.

pub mod linux;
pub mod macos;

/// Where `script/file-types` writes the icons, from the workspace: each
/// format's as `<extension>.svg`, and video's and audio's, without one.
pub const ICONS: &str = "resources/file-types";

/// The names of the icons in `ICONS`: the extension of each format Plays
/// is offered for, then video and audio.
pub fn icon_names() -> Vec<&'static str> {
    FORMATS
        .iter()
        .filter(|format| format.is_claimed())
        .map(Format::extension)
        .chain([Kind::Video.name(), Kind::Audio.name()])
        .collect()
}

/// Whether a format's files are video or audio, which picks its icon.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Kind {
    Video,
    Audio,
}

impl Kind {
    /// The name of its icon without an extension.
    pub fn name(self) -> &'static str {
        match self {
            Self::Video => "video",
            Self::Audio => "audio",
        }
    }
}

/// A format's type ID on macOS.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MacType {
    /// One macOS knows.
    System(&'static str),
    /// One Plays declares, for Macs that don't know it; macOS's own, or
    /// another app's, wins where there is one.
    Declared(&'static str),
}

impl MacType {
    pub fn id(self) -> &'static str {
        match self {
            Self::System(id) | Self::Declared(id) => id,
        }
    }
}

/// A format Plays opens.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Format {
    /// The extensions of its files, lowercase; the first is on its icon.
    pub extensions: &'static [&'static str],
    pub kind: Kind,
    /// What it's called, as Finder's Kind says.
    pub name: &'static str,
    /// The MIME types Linux gives its files (shared-mime-info's names),
    /// then the names older versions gave them.
    pub mime: &'static [&'static str],
    pub mac: Option<MacType>,
}

impl Format {
    /// The extension on its icon, which names the icon's files.
    pub fn extension(&self) -> &'static str {
        self.extensions[0]
    }

    /// Whether Plays is offered for its files on either system, and so
    /// whether it has an icon. Otherwise it's only listed in folders.
    pub fn is_claimed(&self) -> bool {
        !self.mime.is_empty() || self.mac.is_some()
    }
}

/// The format of files with `extension`, in any case.
pub fn format_of(extension: &str) -> Option<&'static Format> {
    FORMATS.iter().find(|format| {
        format
            .extensions
            .iter()
            .any(|known| known.eq_ignore_ascii_case(extension))
    })
}

/// The formats a folder opens to, and a file's previous and next ones are
/// picked from: video, then audio, each by extension.
pub const FORMATS: &[Format] = &[
    video(&["3gp"], "3GPP video", &["video/3gpp"], sys("public.3gpp")),
    video(
        &["avi"],
        "AVI video",
        &["video/vnd.avi", "video/x-msvideo"],
        sys("public.avi"),
    ),
    video(
        &["flv"],
        "Flash video",
        &["video/x-flv"],
        ours("com.adobe.flash.video"),
    ),
    // Plays reads it, but it's an image, and Plays claims none.
    video(&["gif"], "GIF image", &[], None),
    // Typed video/mp2t on Linux, as .ts files are.
    video(
        &["m2ts", "mts"],
        "AVCHD video",
        &[],
        sys("public.avchd-mpeg-2-transport-stream"),
    ),
    // Typed video/mp4 on Linux now; video/x-m4v before.
    video(
        &["m4v"],
        "MPEG-4 video",
        &["video/x-m4v"],
        sys("com.apple.m4v-video"),
    ),
    video(
        &["mkv"],
        "Matroska video",
        &["video/x-matroska"],
        ours("org.matroska.mkv"),
    ),
    video(
        &["mov"],
        "QuickTime movie",
        &["video/quicktime"],
        sys("com.apple.quicktime-movie"),
    ),
    video(
        &["mp4"],
        "MPEG-4 video",
        &["video/mp4"],
        sys("public.mpeg-4"),
    ),
    video(
        &["mpg", "mpeg"],
        "MPEG video",
        &["video/mpeg"],
        sys("public.mpeg"),
    ),
    video(
        &["mxf"],
        "MXF video",
        &["application/mxf"],
        ours("org.smpte.mxf"),
    ),
    video(
        &["ogv"],
        "Ogg video",
        &["video/ogg", "video/x-theora+ogg"],
        ours("org.xiph.ogv"),
    ),
    video(
        &["ts"],
        "MPEG transport stream",
        &["video/mp2t"],
        sys("public.mpeg-2-transport-stream"),
    ),
    // Typed video/mpeg on Linux; no type on macOS.
    video(&["vob"], "DVD video", &[], None),
    video(
        &["webm"],
        "WebM video",
        &["video/webm"],
        ours("org.webmproject.webm"),
    ),
    video(
        &["wmv"],
        "Windows Media video",
        &["video/x-ms-wmv"],
        ours("com.microsoft.windows-media-wmv"),
    ),
    audio(
        &["aac"],
        "AAC audio",
        &["audio/aac"],
        sys("public.aac-audio"),
    ),
    audio(
        &["aiff", "aif"],
        "AIFF audio",
        &["audio/x-aiff"],
        sys("public.aiff-audio"),
    ),
    // ALAC is usually in .m4a; neither system has a type for .alac.
    audio(&["alac"], "ALAC audio", &[], None),
    audio(
        &["ape"],
        "Monkey's Audio",
        &["audio/x-ape"],
        ours("com.monkeysaudio.ape"),
    ),
    audio(
        &["flac"],
        "FLAC audio",
        &["audio/flac", "audio/x-flac"],
        ours("org.xiph.flac"),
    ),
    audio(
        &["m4a"],
        "MPEG-4 audio",
        &["audio/mp4"],
        sys("com.apple.m4a-audio"),
    ),
    audio(
        &["m4b"],
        "MPEG-4 audiobook",
        &["audio/x-m4b"],
        sys("com.apple.protected-mpeg-4-audio-b"),
    ),
    audio(
        &["mka"],
        "Matroska audio",
        &["audio/x-matroska"],
        ours("org.matroska.mka"),
    ),
    audio(&["mp2"], "MP2 audio", &["audio/mp2"], sys("public.mp2")),
    audio(&["mp3"], "MP3 audio", &["audio/mpeg"], sys("public.mp3")),
    // Linux types an Ogg file by what's in it.
    audio(
        &["ogg", "oga"],
        "Ogg audio",
        &[
            "audio/ogg",
            "audio/x-vorbis+ogg",
            "audio/x-flac+ogg",
            "audio/x-speex+ogg",
        ],
        ours("org.xiph.ogg-audio"),
    ),
    audio(
        &["opus"],
        "Opus audio",
        &["audio/x-opus+ogg"],
        ours("org.xiph.opus"),
    ),
    audio(
        &["wav"],
        "WAVE audio",
        &["audio/vnd.wave", "audio/x-wav"],
        sys("com.microsoft.waveform-audio"),
    ),
    audio(
        &["weba"],
        "WebM audio",
        &["audio/webm"],
        ours("org.webmproject.weba"),
    ),
    audio(
        &["wma"],
        "Windows Media audio",
        &["audio/x-ms-wma"],
        ours("com.microsoft.windows-media-wma"),
    ),
    audio(
        &["wv"],
        "WavPack audio",
        &["audio/x-wavpack"],
        ours("com.wavpack.wv"),
    ),
];

const fn video(
    extensions: &'static [&'static str],
    name: &'static str,
    mime: &'static [&'static str],
    mac: Option<MacType>,
) -> Format {
    Format {
        extensions,
        kind: Kind::Video,
        name,
        mime,
        mac,
    }
}

const fn audio(
    extensions: &'static [&'static str],
    name: &'static str,
    mime: &'static [&'static str],
    mac: Option<MacType>,
) -> Format {
    Format {
        kind: Kind::Audio,
        ..video(extensions, name, mime, mac)
    }
}

const fn sys(id: &'static str) -> Option<MacType> {
    Some(MacType::System(id))
}

const fn ours(id: &'static str) -> Option<MacType> {
    Some(MacType::Declared(id))
}

#[cfg(test)]
#[path = "tests/file_types.rs"]
mod tests;
