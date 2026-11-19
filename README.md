# MBX

MBX is a Toybox multiband dynamics plug-in for Apple Silicon macOS. New instances start with three non-overlapping bands split at 120 Hz and 3 kHz. Split bands to work with up to six. Each band offers downward and upward compression and expansion, a soft knee and gain range, attack and release, makeup gain, stereo linking, mid or side processing, and an internal, custom-frequency, or external detector. The native editor shows input, output, and external sidechain spectra, per-band gain, and output peak meters. The VST3 supports mono and stereo, 32-bit and 64-bit audio, host automation, and project recall.

The default preset starts with neutral thresholds. Hover at a frequency and click **+** to split its band. **Split band** in the band list splits the selected band at its logarithmic midpoint. The new band inherits the original dynamics settings. Drag a shared crossover to move both edges, and scroll over an edge to change its slope. **Remove** merges the selected region into an adjoining band. Bypassing a band preserves its range and settings.

The spectrum stays full height as you edit. Select a band to show its controls above the frequency ruler. The panel moves horizontally with the selected band and stays inside the spectrum width. Click empty space, press Escape, or use **Close** to dismiss the controls. **Bands** opens the band list and its gain meters. Command-click or Shift-click to select a group, or drag a selection box on the spectrum. Drag the center handle horizontally to retune crossovers or vertically to change makeup. Drag the curved range limit through the makeup level to change its direction. The shaded area shows the possible range, and a brighter line follows live movement. Drag a knob up or down to adjust it. Hold Shift for fine adjustments, double-click to type, or use Tab and the arrow keys. Option-click or press Home to reset the focused value. Drag the frequency ruler up or down to zoom and sideways to pan. Double-click the ruler to reset it. Use **Undo** and **Redo** to reverse edits made in the open editor.

Hover a band to reach its **Solo**, **Mute**, **Key**, and slope controls on the spectrum. These controls also appear in the band panel. Click to toggle a monitor, or hold it to audition temporarily. Command-click applies a monitor exclusively. Right-click a band to solo it, or Option-right-click to mute it.

Double-click a band center, crossover, or range line to enter the corresponding value. With the spectrum focused, use Left and Right to select bands, Shift plus an arrow to extend selection, Command-A to select all bands, and Enter to split the selected band. S and M toggle solo and mute. F2 edits makeup and 0 resets frequency zoom.

Drag a window edge to resize the editor in a VST3 host. **Size** cycles through the preset sizes that fit the display. On macOS, click **Full screen** to fill the display with MBX. Press Escape or click **Exit full screen** to return to the previous window size.

**Gain Auto** fits the gain display to the band settings. Click it to choose a fixed scale. The pale curve estimates the combined gain of the bands from their current gain readings and crossover shapes. **Pre**, **Post**, and **SC** select analyzer traces. **Freeze** holds their maxima while the meters continue. The vertical output meters show left and right separately or a single mono channel. The scale is in dBFS. A red clip marker latches when a sample peak reaches 0 dBFS. Click the **Peak** readout to clear the held peaks and clip markers.

Open **Detector** or press D with the spectrum focused. The **KEY** lane shows the selected band’s current detector range. Drag either grip horizontally, hold Shift for fine adjustments, or double-click a grip to enter a cutoff. Dragging a band-linked detector starts a custom range from its current cutoffs. An external detector keeps its source when you move a grip. **Key low** and **Key high** edit the same cutoffs.

Use **Source** to select the internal band, an internal custom range, or an external key. For external triggering, route audio to the DAW sidechain input, then choose **Band** or **Custom** under **Key filter**. Click **Key** in the panel header or press K with the spectrum focused to audition the filtered trigger. The white tick on the Threshold knob marks the detector level. An inactive external input supplies silence.

Choose **Minimum** or **Linear** under **Phase** in the footer. Turn on the global lookahead engine to reserve 20 ms of host-compensated latency, then set each band’s preview independently. With the engine off, minimum-phase operation reports zero latency. Paired, modulated state-variable shelves apply gain within each region, and a separate bandpass feeds the RMS detector. At neutral gain the shelves reduce to identity. Stored overlaps from earlier projects and overlapping host automation are resolved into non-overlapping ranges. Screen meters and the FFT analyzer are for monitoring. The analyzer does not affect DSP. The lower and upper crossover slopes are separately adjustable from 6 to 48 dB/oct. Linear phase uses symmetric FIR filters and overlap-save convolution. The linear-phase filter transitions share their frequency weighting at adjoining crossovers. At 48 kHz it reports 3072 samples of latency, plus 960 samples when lookahead is enabled. Switching phase or global lookahead asks the host to reactivate the plug-in and update delay compensation.

## Presets and A/B

Click the preset name to browse the six factory presets and any user presets. Use the arrows beside the name to load the previous or next preset. In the browser, use Up and Down to navigate, Enter to load, and Escape to close. A star marks settings that have changed since loading the named preset.

Use **Save as…** in the browser to store the current settings in `~/Library/Application Support/MBX/Presets`. MBX keeps existing files when a name is already in use.

Click **A** and **B** to compare two settings. The first switch starts from the current settings. Each side keeps its edits and its own undo history while the editor is open. **Copy** replaces the inactive side with the active settings. The VST3 project state stores the active side and its alternate. Switching or loading settings that change phase or global lookahead requests a host delay-compensation update.

## Build and verify

Use a native Rust toolchain and a Steinberg VST3 SDK checkout. The scripts default to `~/lib/vst3sdk`. Set `VST3_SDK_DIR` to override it.

- `./scripts/ci.sh` checks formatting, runs release Clippy and unit tests, and packages the locally signed build. Set `VST3_VALIDATOR` to the path of Steinberg’s validator to verify the packaged plug-in in the same run.
- `./scripts/bundle.sh` writes `dist/MBX.vst3`. Copy the entire bundle into `~/Library/Audio/Plug-Ins/VST3/` for local testing.

The bundle is ad hoc signed for local use. The DSP-only test suite also runs with `cargo test --no-default-features --lib`.

## DAW acceptance

1. Insert MBX on a stereo track and confirm the three default bands. Load a factory preset using the arrow and the browser. Save a user preset and recall it after reopening the editor.
2. Copy A to B, change B’s threshold, and switch back and forth. Save and reopen the project to check both sides and the active selection.
3. Select a low, middle, and high band. Check that the band panel moves horizontally at a fixed height and clears with Escape. Drag a knob with and without Shift, and check that double-click opens numeric entry. Open **Detector** and pull both **KEY** grips in turn. Confirm the audio crossover stays put. Split a band and drag its shared crossover. Drag the curved range through zero and double-click a handle to enter a value. Test short clicks and held audition on the spectrum. Route an external key and check its SC trace, audition, and Freeze.
4. Compare a minimum-phase setting with a linear-phase setting that uses lookahead. Check Ableton’s delay compensation and listen for transients or unexpected level changes.

5. Resize the window horizontally and vertically. Use **Size**, enter **Full screen**, and exit with Escape. Verify the selected band and window size return, and the panel stays above the frequency ruler. Play a stereo signal with different levels on each side, then click **Peak** to clear the hold.
