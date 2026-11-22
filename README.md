# MBX

<a href='https://ko-fi.com/F7X2225YCF' target='_blank'><img height='36' style='border:0px;height:36px;' src='https://storage.ko-fi.com/cdn/kofi6.png?v=6' border='0' alt='Buy Me a Coffee at ko-fi.com' /></a>

MBX is a multiband dynamics VST3 for Apple Silicon Macs. Start with three bands and split up to six. Each band can compress or expand upward or downward and use an internal or external detector.

- Shape crossovers, gain range and thresholds from the spectrum.
- Inspect input, output and sidechain spectra alongside live gain and peak meters.
- Choose minimum- or linear-phase processing, adjust per-band lookahead and compare settings with A/B.

Run `./scripts/ci.sh` to check and build with the Steinberg VST3 SDK. Use `VST3_SDK_DIR` if the SDK is outside `~/lib/vst3sdk`.

[User guide](docs/USER-GUIDE.md) · [Product brief](docs/product-brief.md) · [MIT license](LICENSE)
