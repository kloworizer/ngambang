# Third-party notices

Ngambang is built with the open-source Rust crates below. Each keeps its own
license; Ngambang's own license (MIT with the Commons Clause) does not apply to
them. Where a crate offers a choice ("MIT OR Apache-2.0"), Ngambang uses it
under the MIT License.

Crates marked *build only* run while compiling and are not part of the
shipped executables.

| Crate | Version | License | Source |
|-------|---------|---------|--------|
| equivalent | 1.0.3 | Apache-2.0 OR MIT | https://github.com/indexmap-rs/equivalent |
| foldhash | 0.2.0 | Zlib | https://github.com/orlp/foldhash |
| hashbrown | 0.17.1 | MIT OR Apache-2.0 | https://github.com/rust-lang/hashbrown |
| indexmap | 2.14.2 | Apache-2.0 OR MIT | https://github.com/indexmap-rs/indexmap |
| itoa | 1.0.18 | MIT OR Apache-2.0 | https://github.com/dtolnay/itoa |
| lexopt | 0.3.2 | MIT | https://github.com/blyxxyz/lexopt |
| memchr | 2.8.3 | Unlicense OR MIT | https://github.com/BurntSushi/memchr |
| proc-macro2 *(build only)* | 1.0.107 | MIT OR Apache-2.0 | https://github.com/dtolnay/proc-macro2 |
| quote *(build only)* | 1.0.47 | MIT OR Apache-2.0 | https://github.com/dtolnay/quote |
| serde | 1.0.229 | MIT OR Apache-2.0 | https://github.com/serde-rs/serde |
| serde_core | 1.0.229 | MIT OR Apache-2.0 | https://github.com/serde-rs/serde |
| serde_derive *(build only)* | 1.0.229 | MIT OR Apache-2.0 | https://github.com/serde-rs/serde |
| serde_json | 1.0.154 | MIT OR Apache-2.0 | https://github.com/serde-rs/json |
| serde_spanned | 1.1.2 | MIT OR Apache-2.0 | https://github.com/toml-rs/toml |
| syn *(build only)* | 2.0.119 | MIT OR Apache-2.0 | https://github.com/dtolnay/syn |
| syn *(build only)* | 3.0.9 | MIT OR Apache-2.0 | https://github.com/dtolnay/syn |
| toml | 1.1.8+spec-1.1.0 | MIT OR Apache-2.0 | https://github.com/toml-rs/toml |
| toml_datetime | 1.1.2+spec-1.1.0 | MIT OR Apache-2.0 | https://github.com/toml-rs/toml |
| toml_parser | 1.1.5+spec-1.1.0 | MIT OR Apache-2.0 | https://github.com/toml-rs/toml |
| toml_writer | 1.1.3+spec-1.1.0 | MIT OR Apache-2.0 | https://github.com/toml-rs/toml |
| unicode-ident *(build only)* | 1.0.27 | (MIT OR Apache-2.0) AND Unicode-3.0 | https://github.com/dtolnay/unicode-ident |
| windows | 0.62.2 | MIT OR Apache-2.0 | https://github.com/microsoft/windows-rs |
| windows-collections | 0.3.2 | MIT OR Apache-2.0 | https://github.com/microsoft/windows-rs |
| windows-core | 0.62.2 | MIT OR Apache-2.0 | https://github.com/microsoft/windows-rs |
| windows-future | 0.3.2 | MIT OR Apache-2.0 | https://github.com/microsoft/windows-rs |
| windows-implement *(build only)* | 0.60.2 | MIT OR Apache-2.0 | https://github.com/microsoft/windows-rs |
| windows-interface *(build only)* | 0.59.3 | MIT OR Apache-2.0 | https://github.com/microsoft/windows-rs |
| windows-link | 0.2.1 | MIT OR Apache-2.0 | https://github.com/microsoft/windows-rs |
| windows-numerics | 0.3.1 | MIT OR Apache-2.0 | https://github.com/microsoft/windows-rs |
| windows-result | 0.4.1 | MIT OR Apache-2.0 | https://github.com/microsoft/windows-rs |
| windows-strings | 0.5.1 | MIT OR Apache-2.0 | https://github.com/microsoft/windows-rs |
| windows-threading | 0.2.1 | MIT OR Apache-2.0 | https://github.com/microsoft/windows-rs |
| winnow | 1.0.4 | MIT | https://github.com/winnow-rs/winnow |
| zmij | 1.0.23 | MIT | https://github.com/dtolnay/zmij |

## MIT License

Applies to every crate above licensed under MIT (including dual-licensed
ones). Copyright belongs to each crate's authors as listed in its source.

```
Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
SOFTWARE.
```

## zlib License

Applies to crates above licensed under Zlib.

```
This software is provided 'as-is', without any express or implied
warranty. In no event will the authors be held liable for any damages
arising from the use of this software.

Permission is granted to anyone to use this software for any purpose,
including commercial applications, and to alter it and redistribute it
freely, subject to the following restrictions:

1. The origin of this software must not be misrepresented; you must not
   claim that you wrote the original software. If you use this software
   in a product, an acknowledgment in the product documentation would be
   appreciated but is not required.
2. Altered source versions must be plainly marked as such, and must not be
   misrepresented as being the original software.
3. This notice may not be removed or altered from any source distribution.
```

## Other licenses

- Apache-2.0: <https://www.apache.org/licenses/LICENSE-2.0>
- Unicode-3.0: <https://www.unicode.org/license.txt> (build only)
- Unlicense: <https://unlicense.org>

## Data sources

The Weather & early warnings and Markets modules show data from these
services. Ngambang fetches it on your device; nothing is redistributed with
the program.

| Source | Data | Terms |
|--------|------|-------|
| [Open-Meteo](https://open-meteo.com) | Weather forecasts | [CC BY 4.0](https://creativecommons.org/licenses/by/4.0/) — "Weather data by Open-Meteo.com"; the source is named in the weather slip |
| [GDACS](https://www.gdacs.org) (UN OCHA / European Commission JRC) | Disaster alerts | [GDACS terms of use](https://www.gdacs.org/About/termofuse.aspx) |
| [USGS Earthquake Hazards Program](https://earthquake.usgs.gov) | Earthquake reports | Public domain (U.S. Government work) |
| [Yahoo Finance](https://finance.yahoo.com) | Quotes and symbol search for the Markets watchlist | For personal, non-commercial display; prices may be delayed. Yahoo's [terms of service](https://legal.yahoo.com/us/en/yahoo/terms/otos/index.html) apply; the source is named in the slip |

Calendars are read from the iCal addresses you add yourself; their content
stays on your computer.

## Not bundled

Ngambang uses fonts, the Direct2D/DirectWrite/Composition APIs and NVIDIA's
`nvml.dll` from your Windows installation and drivers; none of them are
shipped with Ngambang.

To regenerate this list: `cargo metadata --filter-platform x86_64-pc-windows-msvc`.
