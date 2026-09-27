# Third-party notices

sciplot is written in Rust, but a large part of its behaviour is translated from Julia code:
[Makie](https://github.com/MakieOrg/Makie.jl) and its backends GLMakie and CairoMakie,
GridLayoutBase, PlotUtils, Julia's Base library and a few smaller packages. Translated code is a
derivative work, and the licenses of these projects require keeping their copyright and permission
notices. This file reproduces them, records where the bundled data (colormaps, palette, named
colors, fonts) comes from, and cites the papers behind some algorithms.

Each license below is copied verbatim from the license file of the upstream version named in its
heading, which is the version sciplot was ported from (the versions match `tools/Manifest.toml`
for the packages it lists). Every translated source file names its upstreams in a `Provenance:`
note in its header.

The upstreams contributed in different ways. Some sciplot files are line-by-line ports, some adapt
an algorithm to a different structure, and some only reproduce default values or documented
behaviour. The "Used in sciplot" line of each section says which.

sciplot itself is licensed under the MIT License (`LICENSE-MIT`). The bundled fonts are under the
GUST Font License (LPPL 1.3c); see [Fonts](#fonts).

## Code

### Makie 0.24.14

- Repository: https://github.com/MakieOrg/Makie.jl (the root package of the Makie monorepo)
- Used in sciplot: most of the crate. Ported or adapted code covers tick formatting, log and minor ticks (`src/ticks/`), Float32 rebasing (`src/transform/`), axis limits, decorations and the Axis3 camera (`src/scene/`), Axis, Axis3, Legend, Colorbar and Label (`src/blocks/`), plot recipes (`src/plots/`), rich-text layout (`src/text/`), colormap sampling and the Wong palette (`src/color/`), marker geometry (`src/render/svg/marker.rs`, `sprite.wgsl`, `markers3d.wgsl`), dash patterns, miter limits and colormap clipping in the renderers (`src/render/`), interactions and tooltips (`src/window/`), and the defaults in `src/theme/`, `src/figure/`, `src/layout/mod.rs`, `src/data/`, `src/units.rs` and `src/style.rs`.
- License (`LICENSE.md` of Makie 0.24.14):

```text
The MIT License (MIT)
Copyright (c) 2018-2021: Simon Danisch, Julius Krumbiegel.

Permission is hereby granted, free of charge, to any person obtaining a copy of this software
and associated documentation files (the "Software"), to deal in the Software without
restriction, including without limitation the rights to use, copy, modify, merge, publish,
distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the
Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or
substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING
BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND
NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM,
DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
```

### GLMakie 0.13.14

- Repository: https://github.com/MakieOrg/Makie.jl (subdirectory `GLMakie/`)
- Used in sciplot: the GPU line shader (`src/render/gpu/pipelines/line.wgsl`, translated from `lines.geom`, `line_segment.geom` and `lines.frag`; arc lengths in `line.rs`), colormap lookup (`src/render/gpu/common.wgsl`), antialiasing radii and heatmap/line behaviour in `sprite.wgsl`, `markers3d.wgsl`, `lines3d.wgsl` and `field.wgsl`, and the closed-loop test in `src/data/points.rs`.
- License (`LICENSE.md` of GLMakie 0.13.14):

```text
The Makie.jl package is licensed under the MIT "Expat" License:

> Copyright (c) 2017: SimonDanisch.
>
> Permission is hereby granted, free of charge, to any person obtaining a copy
> of this software and associated documentation files (the "Software"), to deal
> in the Software without restriction, including without limitation the rights
> to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
> copies of the Software, and to permit persons to whom the Software is
> furnished to do so, subject to the following conditions:
>
> The above copyright notice and this permission notice shall be included in all
> copies or substantial portions of the Software.
>
> THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
> IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
> FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
> AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
> LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
> OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
> SOFTWARE.
>


Icon made by neungstockr from www.flaticon.com
```

### CairoMakie 0.15.14

- Repository: https://github.com/MakieOrg/Makie.jl (subdirectory `CairoMakie/`)
- Used in sciplot: the SVG backend (`src/render/svg/`: multi-color line paths, dash and miter-limit conversion, fill-then-stroke markers, mesh shading and depth sorting in `three_d.rs`), the shading formula in `mesh3d.wgsl`, marker stroking in `sprite.wgsl` and `markers3d.wgsl`, bar stroking in `src/plots/bars.rs`, and the `pt_per_unit` default in `src/units.rs`.
- License (`LICENSE.md` of CairoMakie 0.15.14):

```text
The CairoMakie.jl package is licensed under the MIT "Expat" License:

> Copyright (c) 2017: SimonDanisch.
>
> Permission is hereby granted, free of charge, to any person obtaining a copy
> of this software and associated documentation files (the "Software"), to deal
> in the Software without restriction, including without limitation the rights
> to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
> copies of the Software, and to permit persons to whom the Software is
> furnished to do so, subject to the following conditions:
>
> The above copyright notice and this permission notice shall be included in all
> copies or substantial portions of the Software.
>
> THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
> IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
> FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
> AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
> LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
> OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
> SOFTWARE.
>
```

### GridLayoutBase 0.11.3

- Repository: https://github.com/jkrumbiegel/GridLayoutBase.jl
- Used in sciplot: the grid layout solver, translated function by function (`src/layout/grid.rs`), the layout types (`src/layout/mod.rs`), the block layout request (`src/blocks/mod.rs`) and the row/column size, gap and side semantics of `src/figure/`.
- License (`LICENSE` of GridLayoutBase 0.11.3):

```text
Copyright (c) 2020 Julius Krumbiegel

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

### PlotUtils 1.5.0

- Repository: https://github.com/JuliaPlots/PlotUtils.jl
- Used in sciplot: `optimize_ticks` and its helpers from `src/ticks.jl`, ported line by line in `src/ticks/wilkinson.rs`; the `grays` colormap and the `Blues`, `Reds` and `RdBu` name aliases behind `src/color/cmap_data.rs`.
- As the last line of the license below records, PlotUtils' `src/ticks.jl` was moved from Gadfly.jl, and its original author is Daniel Jones (@dcjones).
- License (`LICENSE.md` of PlotUtils 1.5.0):

```text
The PlotUtils.jl package is licensed under the MIT "Expat" License:

> Copyright (c) 2016: Thomas Breloff*.
>
> Permission is hereby granted, free of charge, to any person obtaining
> a copy of this software and associated documentation files (the
> "Software"), to deal in the Software without restriction, including
> without limitation the rights to use, copy, modify, merge, publish,
> distribute, sublicense, and/or sell copies of the Software, and to
> permit persons to whom the Software is furnished to do so, subject to
> the following conditions:
>
> The above copyright notice and this permission notice shall be
> included in all copies or substantial portions of the Software.
>
> THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND,
> EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF
> MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT.
> IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY
> CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT,
> TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE
> SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.


Some color gradients are released under individual licenses. The licenses for
each of these follow below:

# matplotlib

> The matplotlib gradients were taken from https://github.com/BIDS/colormap/blob/master/colormaps.py
> Here is the licensing note which accompanied this:

>     New Plots colormaps by Nathaniel J. Smith, Stefan van der Walt,
>     and (in the case of viridis) Eric Firing.
>    
>     This file and the colormaps in it are released under the CC0 license /
>     public domain dedication. We would appreciate credit if you use or
>     redistribute these colormaps, but do not impose any legal restrictions.
>    
>     To the extent possible under law, the persons who associated CC0 with
>     mpl-colormaps have waived all copyright and related or neighboring rights
>     to mpl-colormaps.
>    
>     You should have received a copy of the CC0 legalcode along with this
>     work.  If not, see <http://creativecommons.org/publicdomain/zero/1.0/>.


# cmocean

> The MIT License (MIT)
> Copyright (c) 2015 Kristen M. Thyng
> RGB values were taken from https://github.com/matplotlib/cmocean/tree/master/cmocean/rgb

# colorbrewer

> Apache-Style Software License for ColorBrewer software and ColorBrewer Color Schemes
>
> Copyright (c) 2002 Cynthia Brewer, Mark Harrower, and The Pennsylvania State University.
>
> Licensed under the Apache License, Version 2.0 (the "License"); you may not use this file except in compliance with the License.
> You may obtain a copy of the License at
>
> http://www.apache.org/licenses/LICENSE-2.0
>
> Unless required by applicable law or agreed to in writing, software distributed
> under the License is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR
> CONDITIONS OF ANY KIND, either express or implied. See the License for the
> specific language governing permissions and limitations under the License.
>
> This text from my earlier Apache License Version 1.1 also remains in place for guidance on attribution and permissions:
> Redistribution and use in source and binary forms, with or without modification, are permitted provided that the following conditions are met:
> 1. Redistributions as source code must retain the above copyright notice, this list of conditions and the following disclaimer.
> 2. The end-user documentation included with the redistribution, if any, must include the following acknowledgment:
> "This product includes color specifications and designs developed by Cynthia Brewer (http://colorbrewer.org/)."
> Alternately, this acknowledgment may appear in the software itself, if and wherever such third-party acknowledgments normally appear.
> 4. The name "ColorBrewer" must not be used to endorse or promote products derived from this software without prior written permission. For written permission, please contact Cynthia Brewer at cbrewer@psu.edu.
> 5. Products derived from this software may not be called "ColorBrewer", nor may "ColorBrewer" appear in their name, without prior written permission of Cynthia Brewer.
>
> RGB values were taken from http://www.colorbrewer2.org

# colorcet

> The MIT License (MIT)
> Copyright (c) 2015 Peter Kovesi
> These are the perceptually correct color maps designed by Peter Kovesi and
> described in
> Peter Kovesi. Good Colour Maps: How to Design Them. arXiv:1509.03700 [cs.GR] 2015


The file "ticks.jl" was moved from Gadfly.jl, and the original author is Daniel Jones (@dcjones)
```

### Julia 1.12.7 (Base library)

- Repository: https://github.com/JuliaLang/julia
- Used in sciplot: bit-exact ports of the Base numerics that Makie's tick code depends on, in `src/ticks/julia.rs` (`two_mul` and `pow_body` from `base/math.jl`, Tang's `log` from `base/special/log.jl`, `exp_impl` from `base/special/exp.jl`, `eps`, `isapprox` and digit rounding from `base/float.jl` and `base/floatfuncs.jl`, and `TwicePrecision` float ranges from `base/twiceprecision.jl`); the `J_TABLE` and `t_log_Float64` tables, copied verbatim into `src/ticks/julia_tables.rs`; and `mod1`, `isapprox`, `LinRange` and `searchsortedlast` semantics in `src/scene/axis3/`, `src/plots/contour.rs` and `src/plots/streamplot.rs`.
- `src/ticks/format.rs` reproduces the output of Julia's `Base.Ryu` (`base/ryu/`) with Rust's own float formatting; no Ryu code is ported. (Julia states that `base/ryu` derives from https://github.com/ulfjack/ryu under the Boost Software License.)
- License (`LICENSE.md` of Julia 1.12.7; the `THIRDPARTY.md` it links to is in the Julia repository):

```text
MIT License

Copyright (c) 2009-2024: Jeff Bezanson, Stefan Karpinski, Viral B. Shah, and other contributors: https://github.com/JuliaLang/julia/contributors

Permission is hereby granted, free of charge, to any person obtaining
a copy of this software and associated documentation files (the
"Software"), to deal in the Software without restriction, including
without limitation the rights to use, copy, modify, merge, publish,
distribute, sublicense, and/or sell copies of the Software, and to
permit persons to whom the Software is furnished to do so, subject to
the following conditions:

The above copyright notice and this permission notice shall be
included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND,
EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF
MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND
NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE
LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION
OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION
WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.

end of terms and conditions

Please see [THIRDPARTY.md](./THIRDPARTY.md) for license information for other software used in this project.
```

### ColorSchemes 3.31.0

- Repository: https://github.com/JuliaGraphics/ColorSchemes.jl
- Used in sciplot: the data of every built-in colormap except `grays` (`src/color/cmap_data.rs`). [Colormaps](#colormaps) lists each map's source file and attribution.
- License (`LICENSE.md` of ColorSchemes 3.31.0, which also carries the notices of the colormap collections it bundles):

```text
The ColorSchemes package is licensed under the MIT "Expat" License:

> Copyright (c) 2022: cormullion and contributors.
>
> Permission is hereby granted, free of charge, to any person obtaining
> a copy of this software and associated documentation files (the
> "Software"), to deal in the Software without restriction, including
> without limitation the rights to use, copy, modify, merge, publish,
> distribute, sublicense, and/or sell copies of the Software, and to
> permit persons to whom the Software is furnished to do so, subject to
> the following conditions:
>
> The above copyright notice and this permission notice shall be
> included in all copies or substantial portions of the Software.
>
> THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND,
> EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF
> MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT.
> IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY
> CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT,
> TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE
> SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.

## matplotlib license

New matplotlib colormaps by Nathaniel J. Smith, Stefan van der Walt,
and (in the case of viridis) Eric Firing.

This file and the colormaps in it are released under the CC0 license /
public domain dedication. We would appreciate credit if you use or
redistribute these colormaps, but do not impose any legal restrictions.

To the extent possible under law, the persons who associated CC0 with
mpl-colormaps have waived all copyright and related or neighboring rights
to mpl-colormaps.

## colorbrew license

You should have received a copy of the CC0 legalcode along with this
work.  If not, see <http://creativecommons.org/publicdomain/zero/1.0/>.

Apache-Style Software License for ColorBrewer software and ColorBrewer Color Schemes

Copyright (c) 2002 Cynthia Brewer, Mark Harrower, and The Pennsylvania State University.

Licensed under the Apache License, Version 2.0 (the "License"); you may not use this file except in compliance with the License.
You may obtain a copy of the License at

http://www.apache.org/licenses/LICENSE-2.0

Unless required by applicable law or agreed to in writing, software distributed
under the License is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR
CONDITIONS OF ANY KIND, either express or implied. See the License for the
specific language governing permissions and limitations under the License.

## cmocean license

The MIT License (MIT)

Copyright (c) 2015 Kristen M. Thyng

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

see https://github.com/matplotlib/cmocean

## turbo colormap

see https://ai.googleblog.com/2019/08/turbo-improved-rainbow-colormap-for.html
Copyright 2019 Google LLC.
SPDX-License-Identifier: Apache-2.0


## flags

See https://flagpedia.net/about

The flags from which these color schemes were extracted are public domain.

## feathers

License is MIT
https://github.com/shandiya/feathers/tree/main
```

### Colors 0.13.1

- Repository: https://github.com/JuliaGraphics/Colors.jl
- Used in sciplot: the X11 gray levels in the theme presets (`src/theme/presets.rs`), the color-name conventions of `Color::parse` (`src/color/mod.rs`), and the named-color table (`src/color/named.rs`), whose values were checked against Colors' `src/names_data.jl`.
- License (`LICENSE.md` of Colors 0.13.1):

```text
Color.jl is licensed under the MIT License:

> Copyright (c) 2012-2013: Daniel Jones, Jeff Bezanson, and other contributors.

> Permission is hereby granted, free of charge, to any person obtaining
> a copy of this software and associated documentation files (the
> "Software"), to deal in the Software without restriction, including
> without limitation the rights to use, copy, modify, merge, publish,
> distribute, sublicense, and/or sell copies of the Software, and to
> permit persons to whom the Software is furnished to do so, subject to
> the following conditions:
>
> The above copyright notice and this permission notice shall be
> included in all copies or substantial portions of the Software.
>
> THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND,
> EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF
> MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND
> NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE
> LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION
> OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION
> WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
```

### StatsBase 0.34.13

- Repository: https://github.com/JuliaStats/StatsBase.jl
- Used in sciplot: histogram binning (left-closed bins, out-of-range values dropped) and the `:pdf`, `:density` and `:probability` normalizations of `fit(Histogram, ...)` and `normalize!`, in `src/plots/hist.rs`.
- License (`LICENSE.md` of StatsBase 0.34.13):

```text
StatsBase.jl is licensed under the MIT License:

> Copyright (c) 2012-2016: Dahua Lin, Simon Byrne, Andreas Noack,
> Douglas Bates, John Myles White, Simon Kornblith, and other contributors.

> Permission is hereby granted, free of charge, to any person obtaining
> a copy of this software and associated documentation files (the
> "Software"), to deal in the Software without restriction, including
> without limitation the rights to use, copy, modify, merge, publish,
> distribute, sublicense, and/or sell copies of the Software, and to
> permit persons to whom the Software is furnished to do so, subject to
> the following conditions:
>
> The above copyright notice and this permission notice shall be
> included in all copies or substantial portions of the Software.
>
> THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND,
> EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF
> MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND
> NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE
> LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION
> OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION
> WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
```

### Contour 0.6.3

- Repository: https://github.com/JuliaGeometry/Contour.jl
- Used in sciplot: the marching-squares cell cases, saddle rule and edge interpolation in `src/plots/marching.rs` (segment linking restructured).
- License (`LICENSE.md` of Contour 0.6.3):

```text
The Contour.jl package is licensed under the MIT "Expat" License:

> Copyright (c) 2014: Darwin Darakananda and Tomas Lycken
>
> Permission is hereby granted, free of charge, to any person obtaining
> a copy of this software and associated documentation files (the
> "Software"), to deal in the Software without restriction, including
> without limitation the rights to use, copy, modify, merge, publish,
> distribute, sublicense, and/or sell copies of the Software, and to
> permit persons to whom the Software is furnished to do so, subject to
> the following conditions:
>
> The above copyright notice and this permission notice shall be
> included in all copies or substantial portions of the Software.
>
> THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND,
> EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF
> MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT.
> IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY
> CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT,
> TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE
> SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
```

### Showoff.jl (vendored by Makie)

- Repository: https://github.com/dcjones/Showoff.jl (as given in its license)
- Used in sciplot: indirectly. `src/ticks/format.rs` ports Makie's `src/tick_format.jl`, which states that it "vendors the small subset of Showoff.jl that Makie used to depend on". No Showoff version is pinned; the license below is from Showoff 1.1.1 (identical in 1.0.3).
- License (`LICENSE.md` of Showoff 1.1.1):

```text
The Showoff.jl package is licensed under the MIT "Expat" License:

> Copyright (c) 2014--2015: Daniel C. Jones and [other contributors](https://github.com/dcjones/Showoff.jl/graphs/contributors).
>
> Permission is hereby granted, free of charge, to any person obtaining
> a copy of this software and associated documentation files (the
> "Software"), to deal in the Software without restriction, including
> without limitation the rights to use, copy, modify, merge, publish,
> distribute, sublicense, and/or sell copies of the Software, and to
> permit persons to whom the Software is furnished to do so, subject to
> the following conditions:
>
> The above copyright notice and this permission notice shall be
> included in all copies or substantial portions of the Software.
>
> THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND,
> EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF
> MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT.
> IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY
> CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT,
> TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE
> SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
```

### Isoband 0.1.1

- Repository: https://github.com/jkrumbiegel/Isoband.jl
- Used in sciplot: behaviour only. The filled-contour bands of `src/plots/marching.rs` match Isoband's results (band membership, dropped NaN cells, saddle handling), but no Isoband code is translated. Isoband.jl wraps the isoband C library by Claus O. Wilke (https://github.com/wilkelab/isoband; MIT, per the license shipped in isoband_jll 0.2.3), and none of that C code is used either.
- License (`LICENSE` of Isoband 0.1.1):

```text
MIT License

Copyright (c) 2020 Julius Krumbiegel <julius.krumbiegel@gmail.com> and contributors

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

### Format.jl 1.3.7

- Repository: https://github.com/JuliaString/Format.jl
- Used in sciplot: behaviour only. `format_with` in `src/ticks/format.rs` follows Format.jl's Python-style format-spec semantics, which Makie uses for tick format strings; no code is ported.
- License (`LICENSE.md` of Format.jl 1.3.7):

```text
The JuliaString/Format.jl package is licensed under the MIT "Expat" License,
and is based on the JuliaIO/Formatting.jl package, licensed as follows:

> Copyright (c) 2014: Dahua Lin and contributors.
> Portions Copyright (c) 2017: Gandalf Software, Inc. (Scott Paul Jones) and other contributors
>
> Permission is hereby granted, free of charge, to any person obtaining
> a copy of this software and associated documentation files (the
> "Software"), to deal in the Software without restriction, including
> without limitation the rights to use, copy, modify, merge, publish,
> distribute, sublicense, and/or sell copies of the Software, and to
> permit persons to whom the Software is furnished to do so, subject to
> the following conditions:
>
> The above copyright notice and this permission notice shall be
> included in all copies or substantial portions of the Software.
>
> THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND,
> EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF
> MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT.
> IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY
> CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT,
> TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE
> SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
```

### Inigo Quilez: 2D distance functions

- Source: the 2D distance functions article, https://iquilezles.org/articles/distfunctions2d/,
  and the Shadertoy shaders it links: "Box - distance 2D", "Triangle - distance 2D"
  (https://www.shadertoy.com/view/XsXSz4) and "Polygon - distance 2D"
  (https://www.shadertoy.com/view/wdBXRW).
- Used in sciplot: `sd_box`, `sd_tri` and `sd_ngon` in `src/render/gpu/pipelines/sprite.wgsl` and
  `src/render/gpu/pipelines/markers3d.wgsl` are translated from `sdBox`, `sdTriangle` and
  `sdPolygon`.
- License: the article page states no license; the Shadertoy shaders carry the MIT license in
  the header form below (the triangle shader is dated 2014; the box and polygon shaders carry the
  same header with their own years).

```text
The MIT License
Copyright © 2014 Inigo Quilez
Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the "Software"), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions: The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software. THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
```

## Data

### Colormaps

The 12 built-in colormaps are stored in `src/color/cmap_data.rs` as 256-entry tables.
`tools/gen_colormaps.jl` generated them with Makie 0.24.14: `to_colormap` resolves each name
through PlotUtils' `get_colorscheme` (PlotUtils' own `MISC_COLORSCHEMES` first, then its
`COLORSCHEME_ALIASES`, then ColorSchemes), and `interpolated_getindex` samples the result at 256
points. Maps with 256 upstream colors are reproduced exactly; the others are resampled linearly.

Origin and attribution of each map, as the upstream files state them:

- **viridis, magma, inferno, plasma** (256 colors each): ColorSchemes `data/matplotlib.jl`,
  category "matplotlib". `data/matplotliblicense.txt` and ColorSchemes' `LICENSE.md` state: "New
  matplotlib colormaps by Nathaniel J. Smith, Stefan van der Walt, and (in the case of viridis) Eric
  Firing. This file and the colormaps in it are released under the CC0 license / public domain
  dedication. We would appreciate credit if you use or redistribute these colormaps, but do not
  impose any legal restrictions."
- **cividis** (256 colors): ColorSchemes `data/allcolorschemes.jl`, `:cividis`, category
  "general", notes "Cividis color scale for R". ColorSchemes states no author or license for it.
- **turbo** (256 colors): ColorSchemes `data/allcolorschemes.jl`, `:turbo`, category "general",
  notes "turbo, improved rainbow colormap, anton mikhailov". ColorSchemes' `LICENSE.md` ("turbo
  colormap"): "see https://ai.googleblog.com/2019/08/turbo-improved-rainbow-colormap-for.html
  Copyright 2019 Google LLC. SPDX-License-Identifier: Apache-2.0". The Apache License 2.0 text is
  reproduced [at the end of this file](#apache-license-20).
- **grays** (a 2-stop gradient from RGB 0.05 to RGB 0.95): PlotUtils `src/colorschemes.jl`,
  `MISC_COLORSCHEMES[:grays]`. Part of PlotUtils (MIT, see above).
- **Blues, Reds, RdBu** (9, 9 and 11 colors): ColorSchemes `data/colorbrewerschemes.jl`,
  `:Blues_9`, `:Reds_9` and `:RdBu_11` (PlotUtils' `COLORSCHEME_ALIASES` map the short names to
  these), category "colorbrewer2". File header: "This file contains the schemes derived from
  colorbrewer2.org. The contents of that file are covered under the Apache License 2.0, included in
  this distribution under data/." `data/colorbrewerlicense.txt` is headed "Apache-Style Software
  License for ColorBrewer software and ColorBrewer Color Schemes" and states "Copyright (c) 2002
  Cynthia Brewer, Mark Harrower, and The Pennsylvania State University." and "Licensed under the
  Apache License, Version 2.0" (http://www.apache.org/licenses/LICENSE-2.0). The acknowledgment
  that the ColorBrewer license asks for, as quoted in PlotUtils' `LICENSE.md`: "This product
  includes color specifications and designs developed by Cynthia Brewer (http://colorbrewer.org/)."
  The Apache License 2.0 text is reproduced [at the end of this file](#apache-license-20).
- **balance** (256 colors): ColorSchemes `data/cmocean.jl`, category "cmocean".
  `data/cmoceanlicense.txt`: The MIT License (MIT), Copyright (c) 2015 Kristen M. Thyng, "see
  https://github.com/matplotlib/cmocean". The full notice is part of ColorSchemes' license above.
- **coolwarm** (100 colors): ColorSchemes `data/gnu.jl`, `:coolwarm`, category "matplotlib",
  notes "diverging bipolar color map was generated by Kenneth Moreland". ColorSchemes states no
  license for it.

### Default palette

The default categorical palette (`WONG` in `src/color/mod.rs`) is Makie's `wong_colors`
(`src/theming.jl`), which Makie describes as a "Conservative 7-color palette from Points of view:
Color blindness, Bang Wong - Nature Methods":

Bang Wong, "Points of view: Color blindness", *Nature Methods* 8, 441 (2011).
https://www.nature.com/articles/nmeth.1618

### Named colors

`src/color/named.rs` holds the 148 named colors of the W3C CSS Color Module Level 4 (including
`rebeccapurple`). Their values are identical to the SVG/CSS entries of Colors 0.13.1
`src/names_data.jl` ("the union of every color defined in X11 and in SVG, preferring the SVG
definition when they clash"), the table Makie resolves color names through.

### Fonts

`assets/fonts/TeXGyreHerosMakie-{Regular,Bold,Italic,BoldItalic}.otf` are TeX Gyre Heros in the
"TeXGyreHerosMakie" variant that Makie ships, copied unmodified from the Makie assets artifact
`ad4e594b35357bcfafa2ed97db3137382a3f09bb` (`fonts/`) and embedded in the library with
`include_bytes!` (`src/text/mod.rs`). They are licensed under the GUST Font License (LaTeX Project
Public License 1.3c or later). See `assets/fonts/README.md` for the source and checksums and
`assets/fonts/GUST-FONT-LICENSE.txt` for the license.

## Algorithms

- Tick placement (`src/ticks/wilkinson.rs`, via PlotUtils and Gadfly): J. Talbot, S. Lin and
  P. Hanrahan, "An Extension of Wilkinson's Algorithm for Positioning Tick Labels on Axes", *IEEE
  Transactions on Visualization and Computer Graphics* 16(6), 2010 (InfoVis 2010).
- Logarithm (`src/ticks/julia.rs`, via Julia's `base/special/log.jl`): Ping-Tak Peter Tang,
  "Table-driven Implementation of the Logarithm Function in IEEE Floating-point Arithmetic", *ACM
  Transactions on Mathematical Software* 16(4):378-400, 1990. https://doi.org/10.1145/98267.98294
- Streamline seeding (`src/plots/streamplot.rs`, via Makie's `streamplot`): the R2 quasi-random
  sequence, which Makie cites as "Quasirandom sequences",
  http://extremelearning.com.au/unreasonable-effectiveness-of-quasirandom-sequences/
- Marker signed distance functions (`sprite.wgsl`, `markers3d.wgsl`): Inigo Quilez, see
  [above](#inigo-quilez-2d-distance-functions).

## Apache License 2.0

The turbo and ColorBrewer (Blues, Reds, RdBu) colormap data above are licensed under the Apache
License, Version 2.0:

```text
                              Apache License
                        Version 2.0, January 2004
                     http://www.apache.org/licenses/

TERMS AND CONDITIONS FOR USE, REPRODUCTION, AND DISTRIBUTION

1. Definitions.

   "License" shall mean the terms and conditions for use, reproduction,
   and distribution as defined by Sections 1 through 9 of this document.

   "Licensor" shall mean the copyright owner or entity authorized by
   the copyright owner that is granting the License.

   "Legal Entity" shall mean the union of the acting entity and all
   other entities that control, are controlled by, or are under common
   control with that entity. For the purposes of this definition,
   "control" means (i) the power, direct or indirect, to cause the
   direction or management of such entity, whether by contract or
   otherwise, or (ii) ownership of fifty percent (50%) or more of the
   outstanding shares, or (iii) beneficial ownership of such entity.

   "You" (or "Your") shall mean an individual or Legal Entity
   exercising permissions granted by this License.

   "Source" form shall mean the preferred form for making modifications,
   including but not limited to software source code, documentation
   source, and configuration files.

   "Object" form shall mean any form resulting from mechanical
   transformation or translation of a Source form, including but
   not limited to compiled object code, generated documentation,
   and conversions to other media types.

   "Work" shall mean the work of authorship, whether in Source or
   Object form, made available under the License, as indicated by a
   copyright notice that is included in or attached to the work
   (an example is provided in the Appendix below).

   "Derivative Works" shall mean any work, whether in Source or Object
   form, that is based on (or derived from) the Work and for which the
   editorial revisions, annotations, elaborations, or other modifications
   represent, as a whole, an original work of authorship. For the purposes
   of this License, Derivative Works shall not include works that remain
   separable from, or merely link (or bind by name) to the interfaces of,
   the Work and Derivative Works thereof.

   "Contribution" shall mean any work of authorship, including
   the original version of the Work and any modifications or additions
   to that Work or Derivative Works thereof, that is intentionally
   submitted to Licensor for inclusion in the Work by the copyright owner
   or by an individual or Legal Entity authorized to submit on behalf of
   the copyright owner. For the purposes of this definition, "submitted"
   means any form of electronic, verbal, or written communication sent
   to the Licensor or its representatives, including but not limited to
   communication on electronic mailing lists, source code control systems,
   and issue tracking systems that are managed by, or on behalf of, the
   Licensor for the purpose of discussing and improving the Work, but
   excluding communication that is conspicuously marked or otherwise
   designated in writing by the copyright owner as "Not a Contribution."

   "Contributor" shall mean Licensor and any individual or Legal Entity
   on behalf of whom a Contribution has been received by Licensor and
   subsequently incorporated within the Work.

2. Grant of Copyright License. Subject to the terms and conditions of
   this License, each Contributor hereby grants to You a perpetual,
   worldwide, non-exclusive, no-charge, royalty-free, irrevocable
   copyright license to reproduce, prepare Derivative Works of,
   publicly display, publicly perform, sublicense, and distribute the
   Work and such Derivative Works in Source or Object form.

3. Grant of Patent License. Subject to the terms and conditions of
   this License, each Contributor hereby grants to You a perpetual,
   worldwide, non-exclusive, no-charge, royalty-free, irrevocable
   (except as stated in this section) patent license to make, have made,
   use, offer to sell, sell, import, and otherwise transfer the Work,
   where such license applies only to those patent claims licensable
   by such Contributor that are necessarily infringed by their
   Contribution(s) alone or by combination of their Contribution(s)
   with the Work to which such Contribution(s) was submitted. If You
   institute patent litigation against any entity (including a
   cross-claim or counterclaim in a lawsuit) alleging that the Work
   or a Contribution incorporated within the Work constitutes direct
   or contributory patent infringement, then any patent licenses
   granted to You under this License for that Work shall terminate
   as of the date such litigation is filed.

4. Redistribution. You may reproduce and distribute copies of the
   Work or Derivative Works thereof in any medium, with or without
   modifications, and in Source or Object form, provided that You
   meet the following conditions:

   (a) You must give any other recipients of the Work or
       Derivative Works a copy of this License; and

   (b) You must cause any modified files to carry prominent notices
       stating that You changed the files; and

   (c) You must retain, in the Source form of any Derivative Works
       that You distribute, all copyright, patent, trademark, and
       attribution notices from the Source form of the Work,
       excluding those notices that do not pertain to any part of
       the Derivative Works; and

   (d) If the Work includes a "NOTICE" text file as part of its
       distribution, then any Derivative Works that You distribute must
       include a readable copy of the attribution notices contained
       within such NOTICE file, excluding those notices that do not
       pertain to any part of the Derivative Works, in at least one
       of the following places: within a NOTICE text file distributed
       as part of the Derivative Works; within the Source form or
       documentation, if provided along with the Derivative Works; or,
       within a display generated by the Derivative Works, if and
       wherever such third-party notices normally appear. The contents
       of the NOTICE file are for informational purposes only and
       do not modify the License. You may add Your own attribution
       notices within Derivative Works that You distribute, alongside
       or as an addendum to the NOTICE text from the Work, provided
       that such additional attribution notices cannot be construed
       as modifying the License.

   You may add Your own copyright statement to Your modifications and
   may provide additional or different license terms and conditions
   for use, reproduction, or distribution of Your modifications, or
   for any such Derivative Works as a whole, provided Your use,
   reproduction, and distribution of the Work otherwise complies with
   the conditions stated in this License.

5. Submission of Contributions. Unless You explicitly state otherwise,
   any Contribution intentionally submitted for inclusion in the Work
   by You to the Licensor shall be under the terms and conditions of
   this License, without any additional terms or conditions.
   Notwithstanding the above, nothing herein shall supersede or modify
   the terms of any separate license agreement you may have executed
   with Licensor regarding such Contributions.

6. Trademarks. This License does not grant permission to use the trade
   names, trademarks, service marks, or product names of the Licensor,
   except as required for reasonable and customary use in describing the
   origin of the Work and reproducing the content of the NOTICE file.

7. Disclaimer of Warranty. Unless required by applicable law or
   agreed to in writing, Licensor provides the Work (and each
   Contributor provides its Contributions) on an "AS IS" BASIS,
   WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or
   implied, including, without limitation, any warranties or conditions
   of TITLE, NON-INFRINGEMENT, MERCHANTABILITY, or FITNESS FOR A
   PARTICULAR PURPOSE. You are solely responsible for determining the
   appropriateness of using or redistributing the Work and assume any
   risks associated with Your exercise of permissions under this License.

8. Limitation of Liability. In no event and under no legal theory,
   whether in tort (including negligence), contract, or otherwise,
   unless required by applicable law (such as deliberate and grossly
   negligent acts) or agreed to in writing, shall any Contributor be
   liable to You for damages, including any direct, indirect, special,
   incidental, or consequential damages of any character arising as a
   result of this License or out of the use or inability to use the
   Work (including but not limited to damages for loss of goodwill,
   work stoppage, computer failure or malfunction, or any and all
   other commercial damages or losses), even if such Contributor
   has been advised of the possibility of such damages.

9. Accepting Warranty or Additional Liability. While redistributing
   the Work or Derivative Works thereof, You may choose to offer,
   and charge a fee for, acceptance of support, warranty, indemnity,
   or other liability obligations and/or rights consistent with this
   License. However, in accepting such obligations, You may act only
   on Your own behalf and on Your sole responsibility, not on behalf
   of any other Contributor, and only if You agree to indemnify,
   defend, and hold each Contributor harmless for any liability
   incurred by, or claims asserted against, such Contributor by reason
   of your accepting any such warranty or additional liability.

END OF TERMS AND CONDITIONS

APPENDIX: How to apply the Apache License to your work.

   To apply the Apache License to your work, attach the following
   boilerplate notice, with the fields enclosed by brackets "[]"
   replaced with your own identifying information. (Don't include
   the brackets!)  The text should be enclosed in the appropriate
   comment syntax for the file format. We also recommend that a
   file or class name and description of purpose be included on the
   same "printed page" as the copyright notice for easier
   identification within third-party archives.

Copyright [yyyy] [name of copyright owner]

Licensed under the Apache License, Version 2.0 (the "License");
you may not use this file except in compliance with the License.
You may obtain a copy of the License at

	http://www.apache.org/licenses/LICENSE-2.0

Unless required by applicable law or agreed to in writing, software
distributed under the License is distributed on an "AS IS" BASIS,
WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
See the License for the specific language governing permissions and
limitations under the License.
```
