struct VertexInput {
    @location(0) position: vec3<f32>,
};

struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) world_position: vec3<f32>,
};

struct Uniforms {
    @location(0) time: f32,
    @location(1) view_proj: mat4x4<f32>,
};

@group(0) @binding(0) var<uniform> uniforms: Uniforms;

@vertex
@vertex
fn vs_main(model: VertexInput) -> VertexOutput {
    var out: VertexOutput;

    var pos = model.position;

    // Calculate waves using both X and Z coordinates
    let wave1 = sin(pos.x * 2.0 + pos.z * 2.0 + uniforms.time * 2.0) * 0.3;
    let wave2 = sin(pos.z * 1.5 + pos.x * 2.0 + uniforms.time * 1.5) * 0.2;
    let wave3 = sin(pos.x * 3.0 + pos.z * 3.0 + uniforms.time) * 0.1;

    // Apply the combined waves without z_factor dampening
    pos.y += wave1 + wave2 + wave3;

    out.world_position = pos;
    out.clip_position = uniforms.view_proj * vec4<f32>(pos, 1.0);

    return out;
}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    // Calculate depth-based fade
    let depth = 1.0 - (in.clip_position.z / in.clip_position.w);
    let fade = pow(depth, 1.5);

    // Return white color with depth-based fade
    return vec4<f32>(1.0, 1.0, 1.0, fade);
}