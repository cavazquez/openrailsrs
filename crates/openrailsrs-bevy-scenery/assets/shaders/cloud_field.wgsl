fn cloud_hash(p: vec3<f32>) -> f32 {return fract(sin(dot(p,vec3(127.1,311.7,74.7)))*43758.5453);}
fn cloud_noise(p: vec3<f32>) -> f32 {
 let i=floor(p);let f=fract(p);let u=f*f*(3.0-2.0*f);
 let a=mix(mix(cloud_hash(i),cloud_hash(i+vec3(1.,0.,0.)),u.x),mix(cloud_hash(i+vec3(0.,1.,0.)),cloud_hash(i+vec3(1.,1.,0.)),u.x),u.y);
 let b=mix(mix(cloud_hash(i+vec3(0.,0.,1.)),cloud_hash(i+vec3(1.,0.,1.)),u.x),mix(cloud_hash(i+vec3(0.,1.,1.)),cloud_hash(i+vec3(1.,1.,1.)),u.x),u.y);
 return mix(a,b,u.z);
}
// Periodic coordinates preserve phase across float reduction and origin shifts.
fn cloud_coverage(world:vec2<f32>,coverage:f32)->f32 {
 let phase=world*0.000766990394;
 let p=vec3(cos(phase.x),sin(phase.x),cos(phase.y))*3.;
 let q=vec3(cos(phase.y),sin(phase.y),sin(phase.x))*3.;
 let n=cloud_noise(p)*0.5+cloud_noise(q*2.1)*0.3+cloud_noise(p*4.3)*0.2;
 return smoothstep(1.-coverage,1.14-coverage,n);
}
