// Retro overlay fragment pipeline. Mirrors the CPU reference in
// `retro/mod.rs` (`region_at`, `cell_color`, `reduce_color`, `render`):
// same global-grid cells, same stepped lens edge, same Bayer offset, same
// Rec.601-weighted palette distance. Change both together.
#include <metal_stdlib>
using namespace metal;

struct Uniforms {
    float2 size;          // drawable size in physical px
    float  bg_cell;       // background cell (px)
    float  focus_cell;    // focus cell (px), 1 = colour only
    float4 focus;         // focus rect x,y,w,h (px), already grid-snapped
    float2 lens_c;        // lens centre (px)
    float  lens_r;        // lens radius (px)
    float  lens_cell;     // lens cell (px)
    int    focus_on;
    int    lens_mode;     // 0 off, 1 original, 2 cell
    int    reduce_fixed;  // 1 = palette, 0 = channel depth
    int    bits;
    int    pal_n;
    int    bayer_n;       // 0 = off
    float  spread;
    float  dstr;          // dither strength 0..1
    int    scan;
    float  scan_i;
    float  scan_px;
    int    crt;
    float  crt_s;
    float  opacity;
    int    border;
    int    sprite;
    float4 light;
    float4 dark;
    float2 mouse;         // px
    int    win_n;         // stage-2 window frames
    float  title_px;      // title bar height (px)
};

struct WinRect { float4 r; int title_off; int title_len; int focused; int pad; };

struct VOut { float4 pos [[position]]; };

vertex VOut retro_vs(uint vid [[vertex_id]]) {
    float2 p = float2((vid << 1) & 2, vid & 2);
    VOut o; o.pos = float4(p * 2.0 - 1.0, 0.0, 1.0);
    return o;
}

static float snapc(float p, float cell) { return floor(p / cell) * cell; }

static float3 sample_src(texture2d<float> src, sampler s, float2 px, constant Uniforms& u) {
    return src.sample(s, clamp(px / u.size, 0.0, 1.0)).rgb;
}

static float3 reduce(float3 c, float offset, constant Uniforms& u, constant float4* pal) {
    float3 v = c * 255.0 + offset;
    if (u.reduce_fixed == 0) {
        float m = float((1 << u.bits) - 1);
        float3 q = round(clamp(v, 0.0, 255.0) / 255.0 * m) / m;
        return q;
    }
    float best = 1e30; float3 bc = float3(0);
    for (int i = 0; i < u.pal_n; i++) {
        float3 p = pal[i].rgb * 255.0;
        float3 d = v - p;
        float dist = 0.299 * d.r * d.r + 0.587 * d.g * d.g + 0.114 * d.b * d.b;
        if (dist < best) { best = dist; bc = pal[i].rgb; }
    }
    return bc;
}

static float3 cell_color(float2 px, float cell, texture2d<float> src, sampler s,
                         constant Uniforms& u, constant float4* pal, constant float* bayer) {
    float2 o = float2(snapc(px.x, cell), snapc(px.y, cell));
    // texel centre at/just before the middle (retro/mod.rs `sample_offset`)
    float3 c = sample_src(src, s, o + floor(cell * 0.5) + 0.5, u);
    float off = 0.0;
    if (u.bayer_n > 0 && u.dstr > 0.0) {
        int n = u.bayer_n;
        int ix = int(fmod(fmod(o.x / cell, float(n)) + float(n), float(n)));
        int iy = int(fmod(fmod(o.y / cell, float(n)) + float(n), float(n)));
        float t = (bayer[iy * n + ix] + 0.5) / float(n * n) - 0.5;
        off = t * u.spread * u.dstr;
    }
    return reduce(c, off, u, pal);
}

// 5x7 pixel font, ASCII 32..126, one byte per row (low 5 bits).
static int glyph_row(constant uchar* font, int ch, int row) {
    if (ch < 32 || ch > 126) ch = 63;
    return int(font[(ch - 32) * 7 + row]);
}

// Classic arrow sprite 12x19 (1 = outline, 2 = fill).
constant uchar ARROW[19][12] = {
    {1,0,0,0,0,0,0,0,0,0,0,0},{1,1,0,0,0,0,0,0,0,0,0,0},{1,2,1,0,0,0,0,0,0,0,0,0},
    {1,2,2,1,0,0,0,0,0,0,0,0},{1,2,2,2,1,0,0,0,0,0,0,0},{1,2,2,2,2,1,0,0,0,0,0,0},
    {1,2,2,2,2,2,1,0,0,0,0,0},{1,2,2,2,2,2,2,1,0,0,0,0},{1,2,2,2,2,2,2,2,1,0,0,0},
    {1,2,2,2,2,2,2,2,2,1,0,0},{1,2,2,2,2,2,2,2,2,2,1,0},{1,2,2,2,2,2,2,1,1,1,1,1},
    {1,2,2,2,1,2,2,1,0,0,0,0},{1,2,2,1,0,1,2,2,1,0,0,0},{1,2,1,0,0,1,2,2,1,0,0,0},
    {1,1,0,0,0,0,1,2,2,1,0,0},{1,0,0,0,0,0,1,2,2,1,0,0},{0,0,0,0,0,0,0,1,2,1,0,0},
    {0,0,0,0,0,0,0,1,1,0,0,0}
};

fragment float4 retro_fs(VOut in [[stage_in]],
                         texture2d<float> src [[texture(0)]],
                         constant Uniforms& u [[buffer(0)]],
                         constant float4* pal [[buffer(1)]],
                         constant float* bayer [[buffer(2)]],
                         constant WinRect* wins [[buffer(3)]],
                         constant uchar* titles [[buffer(4)]],
                         constant uchar* font [[buffer(5)]]) {
    constexpr sampler s(filter::nearest, address::clamp_to_edge);
    float2 px = in.pos.xy;

    // CRT barrel curvature: remap where we read from.
    if (u.crt != 0) {
        float2 uv = px / u.size - 0.5;
        float k = 0.12 * u.crt_s;
        uv *= 1.0 + k * dot(uv, uv);
        if (abs(uv.x) > 0.5 || abs(uv.y) > 0.5) return float4(0, 0, 0, u.opacity);
        px = (uv + 0.5) * u.size;
    }

    float3 c;
    bool done = false;
    if (u.lens_mode != 0) {
        float2 cc = float2(snapc(px.x, u.bg_cell), snapc(px.y, u.bg_cell)) + u.bg_cell * 0.5;
        if (distance(cc, u.lens_c) <= u.lens_r) {
            c = (u.lens_mode == 1) ? sample_src(src, s, px, u)
                                   : cell_color(px, u.lens_cell, src, s, u, pal, bayer);
            done = true;
        }
    }
    if (!done) {
        bool in_focus = u.focus_on != 0 && px.x >= u.focus.x && px.y >= u.focus.y &&
                        px.x < u.focus.x + u.focus.z && px.y < u.focus.y + u.focus.w;
        c = cell_color(px, in_focus ? u.focus_cell : u.bg_cell, src, s, u, pal, bayer);
        if (in_focus && u.border != 0) {
            float b = max(u.bg_cell, 1.0);
            if (px.x < u.focus.x + b || px.y < u.focus.y + b ||
                px.x >= u.focus.x + u.focus.z - b || px.y >= u.focus.y + u.focus.w - b) {
                c = u.light.rgb;
            }
        }
    }

    // Stage 2: retro dialog frames, front-to-back; the first window that
    // contains the pixel owns it (occlusion).
    float b = max(u.bg_cell, 1.0);
    for (int i = 0; i < u.win_n; i++) {
        float4 r = wins[i].r;
        if (px.x < r.x || px.y < r.y || px.x >= r.x + r.z || px.y >= r.y + r.w) continue;
        bool edge = px.x < r.x + b || px.y < r.y + b || px.x >= r.x + r.z - b || px.y >= r.y + r.w - b;
        if (edge) { c = u.dark.rgb; break; }
        float ty = px.y - (r.y + b);
        if (ty < u.title_px) {
            float3 bar = wins[i].focused != 0 ? u.light.rgb : mix(u.light.rgb, u.dark.rgb, 0.45);
            c = bar;
            // title text, scale so 7 font rows fill ~60 % of the bar
            float sc = max(1.0, floor(u.title_px * 0.6 / 7.0));
            float tx = px.x - (r.x + b + 2.0 * sc);
            float gy = floor((ty - (u.title_px - 7.0 * sc) * 0.5) / sc);
            float gx = floor(tx / sc);
            int chi = int(floor(gx / 6.0));
            int col = int(gx) - chi * 6;
            if (gy >= 0.0 && gy < 7.0 && tx >= 0.0 && chi < wins[i].title_len && col < 5) {
                int ch = int(titles[wins[i].title_off + chi]);
                if ((glyph_row(font, ch, int(gy)) >> (4 - col)) & 1) c = u.dark.rgb;
            }
        }
        break;
    }

    // Sprite cursor
    if (u.sprite != 0) {
        float sc = max(1.0, floor(u.bg_cell * 0.5));
        float2 d = floor((in.pos.xy - u.mouse) / sc);
        if (d.x >= 0.0 && d.y >= 0.0 && d.x < 12.0 && d.y < 19.0) {
            int v = ARROW[int(d.y)][int(d.x)];
            if (v == 1) c = u.dark.rgb; else if (v == 2) c = u.light.rgb;
        }
    }

    float f = 1.0;
    if (u.scan != 0 && int(floor(px.y / max(u.scan_px, 1.0))) % 2 == 1) f *= 1.0 - 0.5 * u.scan_i;
    if (u.crt != 0) {
        float2 d = px / u.size - 0.5;
        f *= 1.0 - u.crt_s * 0.6 * dot(d, d) * 2.0;
    }
    return float4(c * f * u.opacity, u.opacity); // premultiplied
}
