# MBX

MBX is a multiband dynamics VST3 for Apple Silicon Macs. Start with three bands and split up to six, with upward or downward compression and expansion and internal or external detection per band.

- Shape crossovers, gain range, and thresholds directly from the spectrum.
- Inspect input, output, and sidechain spectra alongside live gain and peak meters.
- Choose minimum- or linear-phase processing, adjust per-band lookahead, and compare settings with A/B.

## Build

Requires macOS (Apple Silicon), Rust, and the [Steinberg VST3 SDK](https://github.com/steinbergmedia/vst3sdk) (`VST3_SDK_DIR`, defaults to `~/lib/vst3sdk`).

```sh
VST3_SDK_DIR=/path/to/vst3sdk ./scripts/bundle.sh  # build and sign dist/MBX.vst3
VST3_SDK_DIR=/path/to/vst3sdk ./scripts/ci.sh      # check, test, and bundle
```

## About

I’m [PORTALSURFER](https://soundcloud.com/portalsurfer). I make music and build instruments and tools for making it under [fromvoid](https://github.com/fromvoid-audio). I use AI extensively in that work and share these projects so more people can make music, whatever their budget.

I build this software for myself first and foremost and offer no guarantees of support. That said, I welcome code, documentation, and interesting ideas, and I’m happy to add them when we align on the direction.

Copyright wasn't just inconvenienced by AI; perhaps it was always the wrong bet.

We treated ideas like property, then acted surprised when progress slowed and wealth pooled upward. But evolution has always followed a simple pattern: copying, mutation, and selection. Ideas, like living things, develop through variation and iteration.

Perhaps the mistake was never copying, but our insistence on preventing it.

Soon, software will be something we simply wish into existence, endlessly adapting to our needs. When creation becomes abundant, artificial scarcity loses its purpose.

fromvoid is built in the open, not as charity, but as a refusal to restrict the evolution of ideas.

Copy. Mutate. Select. Repeat.

## Support

If you’d like to help cover development costs, you can chip in on Ko-fi:

<a href='https://ko-fi.com/F7X2225YCF' target='_blank'><img height='36' style='border:0px;height:36px;' src='https://storage.ko-fi.com/cdn/kofi6.png?v=6' border='0' alt='Buy Me a Coffee at ko-fi.com' /></a>

---

[User guide](docs/USER-GUIDE.md) · [Product brief](docs/product-brief.md) · [MIT license](LICENSE)
