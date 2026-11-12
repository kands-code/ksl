#version 330

// Input vertex attributes (from vertex shader)
in vec2 fragTexCoord;
in vec4 fragColor;

// Input uniform values
uniform sampler2D texture0;
uniform vec4 colDiffuse;

// Output fragment color
out vec4 finalColor;

// Size of the world in pixels
uniform vec2 resolution;

void main()
{
    float x = 1.0/resolution.x;
    float y = 1.0/resolution.y;

    // 1 = alive (black), 0 = dead (background)
    int origValue = (texture(texture0, fragTexCoord).r < 0.1) ? 1 : 0;

    int sumValue = (texture(texture0, vec2(fragTexCoord.x - x, fragTexCoord.y - y)).r < 0.1) ? 1 : 0;
    sumValue    += (texture(texture0, vec2(fragTexCoord.x - x, fragTexCoord.y    )).r < 0.1) ? 1 : 0;
    sumValue    += (texture(texture0, vec2(fragTexCoord.x - x, fragTexCoord.y + y)).r < 0.1) ? 1 : 0;

    sumValue    += (texture(texture0, vec2(fragTexCoord.x,     fragTexCoord.y - y)).r < 0.1) ? 1 : 0;
    sumValue    += (texture(texture0, vec2(fragTexCoord.x,     fragTexCoord.y + y)).r < 0.1) ? 1 : 0;

    sumValue    += (texture(texture0, vec2(fragTexCoord.x + x, fragTexCoord.y - y)).r < 0.1) ? 1 : 0;
    sumValue    += (texture(texture0, vec2(fragTexCoord.x + x, fragTexCoord.y    )).r < 0.1) ? 1 : 0;
    sumValue    += (texture(texture0, vec2(fragTexCoord.x + x, fragTexCoord.y + y)).r < 0.1) ? 1 : 0;

    if (((origValue == 1) && (sumValue == 2)) || sumValue == 3)
        finalColor = vec4(0.0, 0.0, 0.0, 1.0);
    else
        finalColor = fragColor;
}
