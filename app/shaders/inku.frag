#version 460 core

#include <flutter/runtime_effect.glsl>

precision lowp float;

// Uniforms (per-shader-invocation inputs)

// The (width, height) of the shader paint area, in pixels.
layout(location = 0) uniform vec2 u_resolution;
// Time, in seconds.
layout(location = 1) uniform float u_time;
// Horizontal scroll offset of the landing page carousel, in global normalized
// coordinates (i.e., center of first screen is (0, 0)).
layout(location = 2) uniform float u_scroll_offset;

// Outputs

// What color this pixel should be (RGBA).
layout(location = 0) out vec4 o_frag_color;

// // This "rose" colormap comes from:
// // [transform - rose colormap](https://github.com/kbinani/colormap-shaders/blob/master/include/colormap/private/transform/rose.h#L50)
//
// float colormap_rose_red(float x) {
//     if (x < 0.0) {
//         return 54.0 / 255.0;
//     } else if (x < 20049.0 / 82979.0) {
//         return (829.79 * x + 54.51) / 255.0;
//     } else {
//         return 1.0;
//     }
// }
//
// float colormap_rose_green(float x) {
//     if (x < 20049.0 / 82979.0) {
//         return 0.0;
//     } else if (x < 327013.0 / 810990.0) {
//         return (8546482679670.0 / 10875673217.0 * x - 2064961390770.0 / 10875673217.0) / 255.0;
//     } else if (x <= 1.0) {
//         return (103806720.0 / 483977.0 * x + 19607415.0 / 483977.0) / 255.0;
//     } else {
//         return 1.0;
//     }
// }
//
// float colormap_rose_blue(float x) {
//     if (x < 0.0) {
//         return 54.0 / 255.0;
//     } else if (x < 7249.0 / 82979.0) {
//         return (829.79 * x + 54.51) / 255.0;
//     } else if (x < 20049.0 / 82979.0) {
//         return 127.0 / 255.0;
//     } else if (x < 327013.0 / 810990.0) {
//         return (792.02249341361393720147485376583 * x - 64.364790735602331034989206222672) / 255.0;
//     } else {
//         return 1.0;
//     }
// }
//
// vec4 colormap_rose(float x) {
//     return vec4(colormap_rose_red(x), colormap_rose_green(x), colormap_rose_blue(x), 1.0);
// }

// a small colormap that approximates our grey colorscheme using three
// piecewise-linear fns per color channel.
//
// (see: `LxColors.greyXXX` in `lib/style.dart`).

float colormap_lexe_red(float x) {
    if (x <= 0.3) {
        return 0.6 * x;
    } else if (x <= 0.86) {
        return 1.2857 * x + -0.2057;
    } else {
        return 0.7143 * x + 0.2857;
    }
}

float colormap_lexe_green(float x) {
    if (x <= 0.28) {
        return 0.6429 * x;
    } else if (x <= 0.82) {
        return 1.3333 * x + -0.1933;
    } else {
        return 0.5556 * x + 0.4444;
    }
}

float colormap_lexe_blue(float x) {
    if (x <= 0.28) {
        return 0.7143 * x;
    } else if (x <= 0.76) {
        return 1.4063 * x + -0.1938;
    } else {
        return 0.5208 * x + 0.4792;
    }
}

vec4 colormap_lexe(float x) {
    return vec4(
        1.0 - colormap_lexe_red(x),
        1.0 - colormap_lexe_blue(x),
        1.0 - colormap_lexe_green(x),
        1.0
    );
}

float hash12(vec2 n) { 
    return fract(sin(dot(n, vec2(12.9898, 4.1414))) * 43758.5453);
}

// float hash12_alt(vec2 p)
// {
//     vec3 p3  = fract(vec3(p.xyx) * .1031);
//     p3 += dot(p3, p3.yzx + 33.33);
//     return fract((p3.x + p3.y) * p3.z);
// }

float noise(vec2 p){
    vec2 ip = floor(p);
    vec2 u = fract(p);
    // u = smoothstep(0.0, 1.0, u);
    // u = u * u * (3.0 - (2.0 * u));

    float res = mix(
        mix(hash12(ip), hash12(ip + vec2(1.0,0.0)) , u.x),
        mix(hash12(ip + vec2(0.0,1.0)), hash12(ip + vec2(1.0,1.0)), u.x),
        u.y
    );
    // return res;
    return res*res;
}

const mat2 mtx = mat2( 0.81,  0.59, -0.61,  0.82 );

float fbm(vec2 p)
{
    // float t = 0.035 * u_time;
    float t = 0.035 * (u_time + (10.0 * u_scroll_offset));

    float f = 0.0;
    f += 0.500000*noise(p + t); p = mtx*p*2.02;
    f += 0.031250*noise(p - t); p = mtx*p*2.01;
    f += 0.250000*noise(p); p = mtx*p*2.03;
    f += 0.125000*noise(p); p = mtx*p*2.01;
    f += 0.062500*noise(p); p = mtx*p*2.04;

    t = abs((2.0 * fract(t)) - 1.0);
    // t = sin(t);
    f += 0.015625*noise(p + t);

    return f/0.96875;
}

float pattern(vec2 p)
{
    return fbm(p + fbm(p + fbm(p)));
}

vec2 rotate(vec2 v, const vec2 around, float rad)
{
    float s = sin(rad);
    float c = cos(rad);
    mat2 m = mat2(c, -s, s, c);
    return (m * (v - around)) + around;
}

void main() {
    // `pos` is the device coordinates of the current pixel, with the origin
    // starting in the bottom-left.
    //
    // pos.x in [0.0, iResolution.x]
    // pos.y in [0.0, iResolution.y]
    //
    // FlutterFragCoord() is like gl_FragCoord.xy but works consistently across
    // skia and impeller rendering backends.
    vec2 pos = FlutterFragCoord().xy;

    // Global normalized screen coordinates for current pixel
    //
    // gp.y in [-1.0, 1.0]
    vec2 gp = ((2.0 * pos) - u_resolution.xy) / u_resolution.y;

    // Translate and rotate view a little while we scroll.
    gp += vec2(0.25 * u_scroll_offset - 0.25, 1.0);
    gp = rotate(gp, vec2(0.5, 1.0), -0.10 * u_scroll_offset);

    // float darken = 0.8;
    float brightness = 0.9;
    // float zoom = 1.5;
    float zoom = 1.5 + (0.25 * u_scroll_offset);
    float gamma = 1.8;

    // Generate a shade value in [0.0, 1.0]
    float shade = pattern(zoom * gp);

    // linear brightness
    shade = brightness * shade;

    // gamma correction
    shade = pow(shade, gamma);
    
    // float shade;
    // if (gp.x < 0.0) {
    //     shade = hash12(floor(zoom * (gp + vec2(0.035 * u_time, 0.0))));
    // } else {
    //     shade = hash12_alt(floor(zoom * (gp + vec2(0.035 * u_time, 0.0))));
    // }

    // Colorize the shade. This is the output color for this pixel.

    // // greyscale
    // o_frag_color = vec4(shade, shade, shade, 1.0);

    // // red-shifted
    // o_frag_color = vec4(shade, 0.85 * shade, 0.90 * shade, 1.0);

    // // rose
    // o_frag_color = colormap_rose(shade);

    // lexe grey
    o_frag_color = colormap_lexe(shade);

    // // compare colormaps
    // if (gp.x < 0.0) {
    //     o_frag_color = colormap_lexe(shade);
    // } else {
    //     o_frag_color = vec4(shade, shade, shade, 1.0);
    // }
}
