# `Cate` Coda

## `Color` Data

A single color in the OKLAB color space.

+ `l` f32

   The lightness of the color, where `0.0` is absolute black
   and `1.0` is absolute white.

+ `a` f32

   The "a" component of the color, which transforms the hue
   from reddish to greenish as the magnitude increases.

+ `b` f32

   The "b" component of the color, which transforms the hue
   from yellowish to blueish as the magnitude increases.

## `ColorSystem` Data

A system of colors derived from 9 base colors.

This set of colors is comprised of:

- One neutral color, with an `L` value of `15`.

- Six primary colors derived from additive (`red`, `green`, and `blue`)
  and subtractive (`cyan`, `magenta`, and `yellow`) color models, with
  LAB lightness values of `55`.

- Two secondary colors (`orange` and `purple`) representing the remaining
  common colors in most systems, with LAB lightness values of `55`.

Seven additional colors are derived from the base neutral color, and two
additional colors are derived from each of the primary and secondary colors,
for a total of 32 colors.

+ `darkest` Color

   A neutral color with an `L` value of `15`.

   Seven additional colors are derived from this color at
   various values of `L`:

   - `20`: `darker`
   - `40`: `dark`
   - `50`: `lightish`
   - `60`: `darkish`
   - `70`: `light`
   - `90`: `lighter`
   - `95`: `lightest`

+ `magenta` Color
+ `red` Color
+ `orange` Color
+ `yellow` Color
+ `green` Color
+ `cyan` Color
+ `blue` Color
+ `purple` Color