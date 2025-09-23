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

- One `neutral` color, with an `L` value of `58`.

- Six primary colors derived from additive (`red`, `green`, and `blue`)
  and subtractive (`cyan`, `magenta`, and `yellow`) color models, with
  LAB lightness values of `58`.

- Two secondary colors (`orange` and `purple`) with LAB lightness values of `58`.

Seven additional colors are derived from the base neutral color,
and two additional colors are derived from each of the primary
and secondary colors, for a total of 32 colors.

+ `neutral` Color
+ `magenta` Color
+ `red` Color
+ `orange` Color
+ `yellow` Color
+ `green` Color
+ `cyan` Color
+ `blue` Color
+ `purple` Color

## `Neutrals` Data

The set of seven neutral colors derived from a `ColorSystem`.

+ `darkest` Color
    `L=19` (`neutral - 39`)

+ `darker` Color
    `L=24` (`neutral - 34`)

+ `dark` Color
    `L=41` (`neutral - 17`)

+ `neutral` Color
    The base neutral, with `L=58`.

+ `light` Color
    `L=75` (`neutral + 17`)

+ `lighter` Color
    `L=92` (`neutral + 34`)

+ `lightest` Color
    `L=97` (`neutral + 39`)