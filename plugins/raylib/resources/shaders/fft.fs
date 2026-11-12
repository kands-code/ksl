#version 330

// 频谱水瀑布: 环形纹理的每一行是一次频谱, 按时间滚动并随年龄变暗.
// NUM_OF_BINS 与 HISTORY_ROWS 需要与示例中的常量一致.

in vec2 fragTexCoord;
in vec4 fragColor;

out vec4 finalColor;

uniform sampler2D iChannel0;
uniform float iWriteRow;

const float NUM_OF_BINS = 64.0;
const float HISTORY_ROWS = 192.0;
const vec3 BACKGROUND = vec3(0.04, 0.05, 0.10);
const float MAX_HUE = 0.78;

// HSV 转 RGB; h, s, v 均在 [0, 1].
vec3 hsv2rgb(vec3 c)
{
    vec4 k = vec4(1.0, 2.0/3.0, 1.0/3.0, 3.0);
    vec3 p = abs(fract(c.xxx + k.xyz)*6.0 - k.www);
    return c.z*mix(k.xxx, clamp(p - k.xxx, 0.0, 1.0), c.y);
}

void main()
{
    float row = mod(iWriteRow - floor(fragTexCoord.y*(HISTORY_ROWS - 1.0)), HISTORY_ROWS);
    float bin = min(floor(fragTexCoord.x*NUM_OF_BINS), NUM_OF_BINS - 1.0);
    float amplitude = texture(iChannel0,
        vec2((bin + 0.5)/NUM_OF_BINS, (row + 0.5)/HISTORY_ROWS)).r;

    // 越靠上越旧, 亮度逐渐衰减.
    float fade = 1.0 - 0.8*fragTexCoord.y;
    float intensity = clamp(amplitude*fade, 0.0, 1.0);

    vec3 rainbow = hsv2rgb(vec3(MAX_HUE*fragTexCoord.x, 0.85, 1.0));
    vec3 color = BACKGROUND + rainbow*intensity;
    finalColor = vec4(clamp(color, 0.0, 1.0), 1.0);
}
