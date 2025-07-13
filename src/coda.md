# `Cate` Coda

## `Color` Data

A single color in the Oklab color space, represented in terms of Oklch.

+ `l` f32
    The lightness of the color, where `0.0` is absolute darkness,
    and `1.0` is the absolute brightest a monitor can display.

+ `c` f32
    The chromaticity of the color, where `0.0` is entirely
    de-saturated and `0.4` is entirely saturated.

+ `h` f32

    The hue of the color, as a value in degrees on an imagined
    color wheel between `0.0` and `360.0`.

## `ColorSystem` Data

A system of 9 base colors.

This set of colors is comprised of:

- One neutral `darkest` color, with an `L` value of `15`.

- Six primary colors derived from additive (`red`, `green`, and `blue`)
  and subtractive (`cyan`, `magenta`, and `yellow`) color models, with
  LAB lightness values of `55`.

- Two secondary colors (`orange` and `purple`) with LAB lightness values of `55`.

Seven additional colors are derived from the base neutral color, and two
additional colors are derived from each of the primary and secondary colors,
for a total of 32 colors.

+ `darkest` Color
+ `magenta` Color
+ `red` Color
+ `orange` Color
+ `yellow` Color
+ `green` Color
+ `cyan` Color
+ `blue` Color
+ `purple` Color

## `Neutrals` Data

The set of eight neutral colors derived from a `ColorSystem`.

+ `darkest` Color
    The base neutral, with `L=15`.

+ `darker` Color
    `L=20`

+ `dark` Color
    `L=40`

+ `darkish` Color
    `L=50`

+ `lightish` Color
    `L=60`

+ `light` Color
    `L=70`

+ `lighter` Color
    `L=90`

+ `lightest` Color
    `L=96`