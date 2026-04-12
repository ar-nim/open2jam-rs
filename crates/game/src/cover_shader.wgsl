struct VertexOutput {
    @builtin(position) clip_position: vec4f,
    @location(0) uv: vec2f,
}

@vertex
fn vs_main(
    @builtin(vertex_index) vertex_index: u32,
) -> VertexOutput {
    let pos = array(
        vec2f(-1.0, -1.0),
        vec2f(1.0, -1.0),
        vec2f(-1.0, 1.0),
        vec2f(1.0, 1.0),
    );

    var p = pos[vertex_index];
    var output: VertexOutput;
    output.clip_position = vec4f(p, 0.0, 1.0);
    // Flip Y for wgpu texture coords (V goes down in screen coords)
    output.uv = vec2f(p.x * 0.5 + 0.5, 1.0 - (p.y * 0.5 + 0.5));
    return output;
}

@group(0) @binding(0)
var tex: texture_2d<f32>;

@group(0) @binding(1)
var samp: sampler;

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4f {
    let color = textureSample(tex, samp, in.uv);
    return vec4f(color.rgb, 1.0);
}