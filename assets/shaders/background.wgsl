// Background pass: a dark gradient behind the world. It gets darker with depth.
// Later versions put parallax rock walls and the sky here.

@vertex
fn vs_background(@builtin(vertex_index) vertex_index: u32) -> @builtin(position) vec4<f32> {
    return fullscreen_position(vertex_index);
}

@fragment
fn fs_background(@builtin(position) position: vec4<f32>) -> @location(0) vec4<f32> {
    let world = frame.view_top_left + position.xy / frame.zoom;
    let size = max(frame.world_cells, vec2<f32>(1.0));
    let depth = clamp(world.y / size.y, 0.0, 1.0);

    let top = vec3<f32>(0.110, 0.140, 0.200);
    let middle = vec3<f32>(0.062, 0.068, 0.090);
    let bottom = vec3<f32>(0.030, 0.028, 0.036);
    var color = mix(top, middle, smoothstep(0.0, 0.55, depth));
    color = mix(color, bottom, smoothstep(0.55, 1.0, depth));

    // A world width of 0 means no limit to the left and right.
    let inside_x = frame.world_cells.x <= 0.0 || (world.x >= 0.0 && world.x < size.x);
    let inside = inside_x && world.y >= 0.0 && world.y < size.y;
    if !inside {
        color *= 0.4;
    }

    // A very small noise stops visible bands in the gradient.
    color += (hash21(position.xy) - 0.5) / 255.0;
    return vec4<f32>(to_output(color), 1.0);
}
