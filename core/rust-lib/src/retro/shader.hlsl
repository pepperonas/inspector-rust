// Retro overlay pixel shader for Windows (D3D11). A 1:1 port of
// `shader.metal`; both mirror the CPU reference in `retro/mod.rs`. The
// cbuffer layout follows HLSL 16-byte register packing and lands on the
// same byte offsets as `GpuUniforms` in `retro/frame.rs` (one struct, both
// platforms). Change all three together.

cbuffer U : register(b0) {
    float2 size;        float bg_cell;     float focus_cell;
    float4 focus;
    float2 lens_c;      float lens_r;      float lens_cell;
    int    focus_on;    int   lens_mode;   int   reduce_fixed; int bits;
    int    pal_n;       int   bayer_n;     float spread;       float dstr;
    int    scan;        float scan_i;      float scan_px;      int crt;
    float  crt_s;       float opacity;     int   border;       int sprite;
    float4 light;
    float4 dark;
    float2 mouse;       int   win_n;       float title_px;
};

struct WinRect { float4 r; int title_off; int title_len; int focused; int pad; };

Texture2D<float4>         src    : register(t0);
StructuredBuffer<float4>  pal    : register(t1);
StructuredBuffer<float>   bayer  : register(t2);
StructuredBuffer<WinRect> wins   : register(t3);
StructuredBuffer<uint>    titles : register(t4);
StructuredBuffer<uint>    font   : register(t5);

float4 retro_vs(uint vid : SV_VertexID) : SV_Position {
    float2 p = float2((vid << 1) & 2, vid & 2);
    return float4(p.x * 2.0 - 1.0, 1.0 - p.y * 2.0, 0.0, 1.0);
}

float snapc(float p, float cell) { return floor(p / cell) * cell; }

// Nearest texel for a drawable position (works for any capture size).
float3 sample_src(float2 px) {
    uint w, h;
    src.GetDimensions(w, h);
    int2 t = int2(floor(px / size * float2(w, h)));
    t = clamp(t, int2(0, 0), int2(int(w) - 1, int(h) - 1));
    return src.Load(int3(t, 0)).rgb;
}

float3 reduce_c(float3 c, float offset) {
    float3 v = c * 255.0 + offset;
    if (reduce_fixed == 0) {
        float m = float((1 << bits) - 1);
        return round(clamp(v, 0.0, 255.0) / 255.0 * m) / m;
    }
    float best = 1e30; float3 bc = float3(0, 0, 0);
    for (int i = 0; i < pal_n; i++) {
        float3 p = pal[i].rgb * 255.0;
        float3 d = v - p;
        float dist = 0.299 * d.r * d.r + 0.587 * d.g * d.g + 0.114 * d.b * d.b;
        if (dist < best) { best = dist; bc = pal[i].rgb; }
    }
    return bc;
}

float3 cell_color(float2 px, float cell) {
    float2 o = float2(snapc(px.x, cell), snapc(px.y, cell));
    float3 c = sample_src(o + floor(cell * 0.5) + 0.5);
    float off = 0.0;
    if (bayer_n > 0 && dstr > 0.0) {
        int n = bayer_n;
        int ix = int(fmod(fmod(o.x / cell, float(n)) + float(n), float(n)));
        int iy = int(fmod(fmod(o.y / cell, float(n)) + float(n), float(n)));
        float t = (bayer[iy * n + ix] + 0.5) / float(n * n) - 0.5;
        off = t * spread * dstr;
    }
    return reduce_c(c, off);
}

int glyph_row(int ch, int row) {
    if (ch < 32 || ch > 126) ch = 63;
    return int(font[(ch - 32) * 7 + row]);
}

static const uint ARROW[19 * 12] = {
    1,0,0,0,0,0,0,0,0,0,0,0, 1,1,0,0,0,0,0,0,0,0,0,0, 1,2,1,0,0,0,0,0,0,0,0,0,
    1,2,2,1,0,0,0,0,0,0,0,0, 1,2,2,2,1,0,0,0,0,0,0,0, 1,2,2,2,2,1,0,0,0,0,0,0,
    1,2,2,2,2,2,1,0,0,0,0,0, 1,2,2,2,2,2,2,1,0,0,0,0, 1,2,2,2,2,2,2,2,1,0,0,0,
    1,2,2,2,2,2,2,2,2,1,0,0, 1,2,2,2,2,2,2,2,2,2,1,0, 1,2,2,2,2,2,2,1,1,1,1,1,
    1,2,2,2,1,2,2,1,0,0,0,0, 1,2,2,1,0,1,2,2,1,0,0,0, 1,2,1,0,0,1,2,2,1,0,0,0,
    1,1,0,0,0,0,1,2,2,1,0,0, 1,0,0,0,0,0,1,2,2,1,0,0, 0,0,0,0,0,0,0,1,2,1,0,0,
    0,0,0,0,0,0,0,1,1,0,0,0
};

float4 retro_ps(float4 pos : SV_Position) : SV_Target {
    float2 px = pos.xy;
    float2 raw = px;
    // Holes (before the CRT warp): the active window as the real window
    // (focus_cell <= 0) and the lens in "original" — transparent.
    bool native = focus_on != 0 && focus_cell <= 0.0;
    if (native && raw.x >= focus.x && raw.y >= focus.y &&
        raw.x < focus.x + focus.z && raw.y < focus.y + focus.w) {
        return float4(0, 0, 0, 0);
    }
    if (lens_mode == 1) {
        float2 lc = float2(snapc(raw.x, bg_cell), snapc(raw.y, bg_cell)) + bg_cell * 0.5;
        if (distance(lc, lens_c) <= lens_r) return float4(0, 0, 0, 0);
    }
    if (crt != 0) {
        float2 uv = px / size - 0.5;
        float k = 0.12 * crt_s;
        uv *= 1.0 + k * dot(uv, uv);
        if (abs(uv.x) > 0.5 || abs(uv.y) > 0.5) return float4(0, 0, 0, opacity);
        px = (uv + 0.5) * size;
    }

    float3 c = float3(0, 0, 0);
    bool done = false;
    if (lens_mode == 2) {
        float2 cc = float2(snapc(px.x, bg_cell), snapc(px.y, bg_cell)) + bg_cell * 0.5;
        if (distance(cc, lens_c) <= lens_r) {
            c = cell_color(px, lens_cell);
            done = true;
        }
    }
    if (!done) {
        bool in_focus = !native && focus_on != 0 && px.x >= focus.x && px.y >= focus.y &&
                        px.x < focus.x + focus.z && px.y < focus.y + focus.w;
        c = cell_color(px, in_focus ? focus_cell : bg_cell);
        if (native && border != 0) {
            // Frame OUTSIDE the real window — its content stays untouched.
            float fb = max(bg_cell, 1.0);
            if (raw.x >= focus.x - fb && raw.y >= focus.y - fb &&
                raw.x < focus.x + focus.z + fb && raw.y < focus.y + focus.w + fb) {
                c = light.rgb;
            }
        }
        if (in_focus && border != 0) {
            float b = max(bg_cell, 1.0);
            if (px.x < focus.x + b || px.y < focus.y + b ||
                px.x >= focus.x + focus.z - b || px.y >= focus.y + focus.w - b) {
                c = light.rgb;
            }
        }
    }

    float bw = max(bg_cell, 1.0);
    for (int i = 0; i < win_n; i++) {
        float4 r = wins[i].r;
        if (px.x < r.x || px.y < r.y || px.x >= r.x + r.z || px.y >= r.y + r.w) continue;
        bool edge = px.x < r.x + bw || px.y < r.y + bw || px.x >= r.x + r.z - bw || px.y >= r.y + r.w - bw;
        if (edge) { c = dark.rgb; break; }
        float ty = px.y - (r.y + bw);
        if (ty < title_px) {
            c = wins[i].focused != 0 ? light.rgb : lerp(light.rgb, dark.rgb, 0.45);
            float sc = max(1.0, floor(title_px * 0.6 / 7.0));
            float tx = px.x - (r.x + bw + 2.0 * sc);
            float gy = floor((ty - (title_px - 7.0 * sc) * 0.5) / sc);
            float gx = floor(tx / sc);
            int chi = int(floor(gx / 6.0));
            int col = int(gx) - chi * 6;
            if (gy >= 0.0 && gy < 7.0 && tx >= 0.0 && chi < wins[i].title_len && col < 5) {
                int ch = int(titles[wins[i].title_off + chi]);
                if ((glyph_row(ch, int(gy)) >> (4 - col)) & 1) c = dark.rgb;
            }
        }
        break;
    }

    if (sprite != 0) {
        float sc = max(1.0, floor(bg_cell * 0.5));
        float2 d = floor((pos.xy - mouse) / sc);
        if (d.x >= 0.0 && d.y >= 0.0 && d.x < 12.0 && d.y < 19.0) {
            uint v = ARROW[int(d.y) * 12 + int(d.x)];
            if (v == 1) c = dark.rgb; else if (v == 2) c = light.rgb;
        }
    }

    float f = 1.0;
    if (scan != 0 && (int(floor(px.y / max(scan_px, 1.0))) % 2) == 1) f *= 1.0 - 0.5 * scan_i;
    if (crt != 0) {
        float2 d = px / size - 0.5;
        f *= 1.0 - crt_s * 0.6 * dot(d, d) * 2.0;
    }
    return float4(c * f * opacity, opacity); // premultiplied
}
