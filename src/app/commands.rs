//! What the app's actions and the playback shortcuts do.

use gpui::App;

use super::dispatch::{with_active, with_active_in_window, with_workspace};
use super::{
    CloseAll, CloseWindow, OpenFiles, OpenSettings, OpenUrl, Quit, SetAspectRatio, SetCrop,
};
use crate::hotkeys::*;
use crate::media::tracks::TrackKind;
use crate::settings::{SUBTITLE_POSITION_RANGE, SUBTITLE_SIZE_RANGE, Settings, SubtitleStyle};
use crate::ui::settings::SettingsView;

pub(super) fn register(cx: &mut App) {
    cx.on_action(|_: &Quit, cx| cx.quit());
    // Deferred, as in `with_workspace`, so the window in front (Settings,
    // say) can be reached.
    cx.on_action(|_: &OpenSettings, cx| cx.defer(SettingsView::open_window));
    cx.on_action(|_: &CloseWindow, cx| {
        cx.defer(|cx| {
            if let Some(window) = cx.active_window() {
                window
                    .update(cx, |_, window, _| window.remove_window())
                    .inspect_err(|err| log::warn!("can't close the window: {err:#}"))
                    .ok();
            }
        })
    });
    cx.on_action(|_: &OpenFiles, cx| {
        with_workspace(cx, |workspace, window, cx| {
            window.activate_window();
            workspace.prompt_open(window, cx);
        })
    });
    cx.on_action(|_: &OpenUrl, cx| {
        with_workspace(cx, |workspace, window, cx| {
            window.activate_window();
            workspace.show_open_url(window, cx);
        })
    });
    cx.on_action(|_: &CloseAll, cx| with_workspace(cx, |w, _, cx| w.close_all(cx)));

    // The tile in front.
    cx.on_action(|_: &PlayPause, cx| with_active(cx, |p, cx| p.play_pause(cx)));
    cx.on_action(|_: &SeekForward, cx| {
        let step = seek_step(cx);
        with_active(cx, move |p, cx| p.seek_by(step, cx))
    });
    cx.on_action(|_: &SeekBackward, cx| {
        let step = seek_step(cx);
        with_active(cx, move |p, cx| p.seek_by(-step, cx))
    });
    cx.on_action(|_: &NextFrame, cx| with_active(cx, |p, cx| p.step_frame(1.0, cx)));
    cx.on_action(|_: &PreviousFrame, cx| with_active(cx, |p, cx| p.step_frame(-1.0, cx)));
    cx.on_action(|_: &Restart, cx| with_active(cx, |p, cx| p.restart(cx)));
    cx.on_action(|_: &PreviousFile, cx| with_active(cx, |p, cx| p.open_neighbor(-1, cx)));
    cx.on_action(|_: &NextFile, cx| with_active(cx, |p, cx| p.open_neighbor(1, cx)));
    cx.on_action(|_: &ToggleLoop, cx| with_active(cx, |p, cx| p.toggle_loop(cx)));
    cx.on_action(|_: &VolumeUp, cx| with_active(cx, |p, cx| p.change_volume(0.05, cx)));
    cx.on_action(|_: &VolumeDown, cx| with_active(cx, |p, cx| p.change_volume(-0.05, cx)));
    cx.on_action(|_: &ToggleMute, cx| with_active(cx, |p, cx| p.toggle_mute(cx)));
    cx.on_action(|_: &NextAudioTrack, cx| {
        with_active(cx, |p, cx| p.next_track(TrackKind::Audio, cx))
    });
    cx.on_action(|_: &NextSubtitleTrack, cx| {
        with_active(cx, |p, cx| p.next_track(TrackKind::Subtitle, cx))
    });
    cx.on_action(|_: &NextVideoTrack, cx| {
        with_active(cx, |p, cx| p.next_track(TrackKind::Video, cx))
    });
    cx.on_action(|_: &AudioEarlier, cx| with_active(cx, |p, cx| p.shift_audio(-0.05, cx)));
    cx.on_action(|_: &AudioLater, cx| with_active(cx, |p, cx| p.shift_audio(0.05, cx)));
    cx.on_action(|_: &ResetAudioDelay, cx| with_active(cx, |p, cx| p.set_audio_delay(0.0, cx)));
    cx.on_action(|_: &SubtitlesEarlier, cx| with_active(cx, |p, cx| p.shift_subtitles(-0.1, cx)));
    cx.on_action(|_: &SubtitlesLater, cx| with_active(cx, |p, cx| p.shift_subtitles(0.1, cx)));
    cx.on_action(|_: &ResetSubtitleDelay, cx| {
        with_active(cx, |p, cx| p.set_subtitle_delay(0.0, cx))
    });
    cx.on_action(|_: &ToggleSubtitles, cx| with_active(cx, |p, cx| p.toggle_subtitles(cx)));
    cx.on_action(|_: &BiggerSubtitles, cx| change_subtitle_size(cx, 0.1));
    cx.on_action(|_: &SmallerSubtitles, cx| change_subtitle_size(cx, -0.1));
    cx.on_action(|_: &SubtitlesUp, cx| change_subtitle_height(cx, 0.01));
    cx.on_action(|_: &SubtitlesDown, cx| change_subtitle_height(cx, -0.01));
    cx.on_action(|_: &RotateRight, cx| with_active(cx, |p, cx| p.rotate(1, cx)));
    cx.on_action(|_: &RotateLeft, cx| with_active(cx, |p, cx| p.rotate(-1, cx)));
    cx.on_action(|_: &FlipHorizontally, cx| with_active(cx, |p, cx| p.flip(true, cx)));
    cx.on_action(|_: &FlipVertically, cx| with_active(cx, |p, cx| p.flip(false, cx)));
    cx.on_action(|_: &NextAspectRatio, cx| with_active(cx, |p, cx| p.next_aspect_ratio(cx)));
    cx.on_action(|_: &NextCrop, cx| with_active(cx, |p, cx| p.next_crop(cx)));
    cx.on_action(|_: &ResetPicture, cx| with_active(cx, |p, cx| p.reset_picture(cx)));
    cx.on_action(|_: &TakeScreenshot, cx| {
        with_active_in_window(cx, |p, window, cx| p.take_screenshot(window, cx))
    });
    cx.on_action(|action: &SetAspectRatio, cx| {
        let aspect = action.0;
        with_active(cx, move |p, cx| p.set_aspect_ratio(aspect, cx))
    });
    cx.on_action(|action: &SetCrop, cx| {
        let crop = action.0;
        with_active(cx, move |p, cx| p.set_crop(crop, cx))
    });
    cx.on_action(|_: &ToggleFullScreen, cx| {
        with_workspace(cx, |_, window, _| window.toggle_fullscreen())
    });
    cx.on_action(|_: &ExitFullScreen, cx| {
        with_workspace(cx, |_, window, _| {
            if window.is_fullscreen() {
                window.toggle_fullscreen();
            }
        })
    });

    // Every tile in the window in front.
    cx.on_action(|_: &PlayPauseAll, cx| with_workspace(cx, |w, _, cx| w.toggle_all(cx)));
    cx.on_action(|_: &PlayAll, cx| with_workspace(cx, |w, _, cx| w.play_all(cx)));
    cx.on_action(|_: &PauseAll, cx| with_workspace(cx, |w, _, cx| w.pause_all(cx)));
    cx.on_action(|_: &RestartAll, cx| with_workspace(cx, |w, _, cx| w.restart_all(cx)));
    cx.on_action(|_: &SeekForwardAll, cx| with_workspace(cx, |w, _, cx| w.seek_all(1.0, cx)));
    cx.on_action(|_: &SeekBackwardAll, cx| with_workspace(cx, |w, _, cx| w.seek_all(-1.0, cx)));
    cx.on_action(|_: &ToggleMuteAll, cx| with_workspace(cx, |w, _, cx| w.toggle_mute_all(cx)));
}

fn seek_step(cx: &App) -> f64 {
    f64::from(cx.global::<Settings>().seek_step_seconds)
}

/// Change the subtitle style for every file, and say so on the tile in front.
fn change_subtitles(cx: &mut App, change: impl FnOnce(&mut SubtitleStyle) -> String) {
    let mut note = String::new();
    Settings::update(cx, true, |settings| {
        let mut style = settings.subtitles.clamped();
        note = change(&mut style);
        settings.subtitles = style;
    });
    with_active(cx, move |p, cx| p.note(note, cx));
}

fn change_subtitle_size(cx: &mut App, delta: f32) {
    change_subtitles(cx, |style| {
        let size = ((style.size + delta) * 20.0).round() / 20.0;
        style.size = size.clamp(*SUBTITLE_SIZE_RANGE.start(), *SUBTITLE_SIZE_RANGE.end());
        format!("Subtitle size {}%", (style.size * 100.0).round())
    });
}

fn change_subtitle_height(cx: &mut App, delta: f32) {
    change_subtitles(cx, |style| {
        let position = ((style.position + delta) * 100.0).round() / 100.0;
        style.position = position.clamp(
            *SUBTITLE_POSITION_RANGE.start(),
            *SUBTITLE_POSITION_RANGE.end(),
        );
        format!("Subtitle height {}%", (style.position * 100.0).round())
    });
}
