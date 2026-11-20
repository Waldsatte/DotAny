// Painting the picture: the sliders run on the graphics card, with a slower path for machines without one.
// The arithmetic is the same as `adjust_pixels` in shared/picture/src/ops.rs, so the file comes out as it looks here.

const VS = `#version 300 es
in vec2 p; out vec2 uv; void main(){ uv=vec2(p.x,-p.y)*.5; gl_Position=vec4(p,0.,1.); }`;
const FS = `#version 300 es
precision highp float; precision highp sampler3D; in vec2 uv; out vec4 o; uniform sampler2D t; uniform sampler3D lut; uniform mat2 g; uniform bool wide; uniform float e,c,a,k,h,d;
float ss(float x0,float x1,float x){ float q=clamp((x-x0)/(x1-x0),0.,1.); return q*q*(3.-2.*q); }
void main(){ vec4 q=texture(t,g*uv+.5); vec3 v=q.rgb;
  v=pow(pow(v,vec3(2.2))*pow(2.,e*1.5),vec3(1./2.2));
  v.r+=k*.12; v.b-=k*.12;
  v=(v-.5)*(c>=0.?1.+c:1.+c*.7)+.5;
  float l=dot(v,vec3(.2126,.7152,.0722));
  v+=h*.28*ss(.45,1.,l)+d*.28*(1.-ss(0.,.55,l));
  l=dot(v,vec3(.2126,.7152,.0722));
  v=mix(vec3(l),v,1.+a);
  v=clamp(v,0.,1.);
  if(wide) v=texture(lut,v*(32./33.)+.5/33.).rgb;
  o=vec4(v*q.a,q.a); }`;

let gl = null,
  glU = {},
  canvas = null,
  cpu = null,
  cpuSrc = null,
  cpuLut = null,
  tw = 0,
  th = 0;
const clamp = (v, a, b) => Math.min(b, Math.max(a, v));

export function initRender(base, noGl) {
  canvas = base;
  if (noGl) return false;
  try {
    gl = base.getContext('webgl2', { preserveDrawingBuffer: true, premultipliedAlpha: true, antialias: false });
    if (!gl) return false;
    const sh = (type, src) => {
      const o = gl.createShader(type);
      gl.shaderSource(o, src);
      gl.compileShader(o);
      return o;
    };
    const pr = gl.createProgram();
    gl.attachShader(pr, sh(gl.VERTEX_SHADER, VS));
    gl.attachShader(pr, sh(gl.FRAGMENT_SHADER, FS));
    gl.linkProgram(pr);
    if (!gl.getProgramParameter(pr, gl.LINK_STATUS)) {
      gl = null;
      return false;
    }
    gl.useProgram(pr);
    const b = gl.createBuffer();
    gl.bindBuffer(gl.ARRAY_BUFFER, b);
    gl.bufferData(gl.ARRAY_BUFFER, new Float32Array([-1, -1, 1, -1, -1, 1, 1, 1]), gl.STATIC_DRAW);
    const loc = gl.getAttribLocation(pr, 'p');
    gl.enableVertexAttribArray(loc);
    gl.vertexAttribPointer(loc, 2, gl.FLOAT, false, 0, 0);
    gl.activeTexture(gl.TEXTURE1);
    gl.bindTexture(gl.TEXTURE_3D, gl.createTexture());
    for (const [p, v] of [
      [gl.TEXTURE_MIN_FILTER, gl.LINEAR],
      [gl.TEXTURE_MAG_FILTER, gl.LINEAR],
      [gl.TEXTURE_WRAP_S, gl.CLAMP_TO_EDGE],
      [gl.TEXTURE_WRAP_T, gl.CLAMP_TO_EDGE],
      [gl.TEXTURE_WRAP_R, gl.CLAMP_TO_EDGE],
    ])
      gl.texParameteri(gl.TEXTURE_3D, p, v);
    gl.activeTexture(gl.TEXTURE0);
    gl.bindTexture(gl.TEXTURE_2D, gl.createTexture());
    gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MIN_FILTER, gl.NEAREST);
    gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MAG_FILTER, gl.NEAREST);
    gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_S, gl.CLAMP_TO_EDGE);
    gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_T, gl.CLAMP_TO_EDGE);
    gl.pixelStorei(gl.UNPACK_ALIGNMENT, 1);
    for (const n of ['t', 'lut', 'g', 'wide', 'e', 'c', 'a', 'k', 'h', 'd']) glU[n] = gl.getUniformLocation(pr, n);
    gl.uniform1i(glU.t, 0);
    gl.uniform1i(glU.lut, 1);
    gl.uniform1i(glU.wide, 0);
    return true;
  } catch {
    gl = null;
    return false;
  }
}
export const usesGpu = () => !!gl;

// the upright picture, or a smaller copy of it. Turning happens when drawing, so a turn costs nothing
export function setPixels(data, w, h) {
  tw = w;
  th = h;
  if (gl) {
    gl.activeTexture(gl.TEXTURE0);
    gl.texImage2D(gl.TEXTURE_2D, 0, gl.RGBA8, w, h, 0, gl.RGBA, gl.UNSIGNED_BYTE, data);
    return;
  }
  cpuSrc = document.createElement('canvas');
  cpuSrc.width = w;
  cpuSrc.height = h;
  cpuSrc
    .getContext('2d')
    .putImageData(new ImageData(new Uint8ClampedArray(data.buffer, data.byteOffset, w * h * 4), w, h), 0, 0);
}

// a wide-gamut picture's numbers, converted to what the screen is shown
export function setLut(table) {
  if (gl) {
    gl.uniform1i(glU.wide, table ? 1 : 0);
    if (table) {
      gl.activeTexture(gl.TEXTURE1);
      gl.texImage3D(gl.TEXTURE_3D, 0, gl.RGBA8, 33, 33, 33, 0, gl.RGBA, gl.UNSIGNED_BYTE, table);
      gl.activeTexture(gl.TEXTURE0);
    }
  } else cpuLut = table;
}

const ZERO = { exposure: 0, contrast: 0, saturation: 0, temperature: 0, highlights: 0, shadows: 0 };
const isZero = (A) => Object.values(A).every((v) => !v);
export function cpuAdjustData(p, A) {
  if (isZero(A)) return;
  const ss = (a, b, x) => {
    const t = clamp((x - a) / (b - a), 0, 1);
    return t * t * (3 - 2 * t);
  };
  const E = 2 ** ((A.exposure / 100) * 1.5),
    C = A.contrast >= 0 ? 1 + A.contrast / 100 : 1 + (A.contrast / 100) * 0.7,
    K = (A.temperature / 100) * 0.12,
    H = (A.highlights / 100) * 0.28,
    D = (A.shadows / 100) * 0.28,
    S = 1 + A.saturation / 100;
  const up = new Float32Array(256);
  for (let i = 0; i < 256; i++) up[i] = ((i / 255) ** 2.2 * E) ** (1 / 2.2);
  for (let i = 0; i < p.length; i += 4) {
    let r = up[p[i]] + K,
      gg = up[p[i + 1]],
      b = up[p[i + 2]] - K;
    r = (r - 0.5) * C + 0.5;
    gg = (gg - 0.5) * C + 0.5;
    b = (b - 0.5) * C + 0.5;
    let l = 0.2126 * r + 0.7152 * gg + 0.0722 * b;
    const add = H * ss(0.45, 1, l) + D * (1 - ss(0, 0.55, l));
    r += add;
    gg += add;
    b += add;
    l = 0.2126 * r + 0.7152 * gg + 0.0722 * b;
    p[i] = (l + (r - l) * S) * 255 + 0.5;
    p[i + 1] = (l + (gg - l) * S) * 255 + 0.5;
    p[i + 2] = (l + (b - l) * S) * 255 + 0.5;
  }
}

// a small canvas through the sliders, for the film strip
export function adjustSmall(src, A) {
  const out = document.createElement('canvas');
  out.width = src.width;
  out.height = src.height;
  const g = out.getContext('2d', { willReadFrequently: true });
  g.drawImage(src, 0, 0);
  if (!isZero(A)) {
    const d = g.getImageData(0, 0, out.width, out.height);
    cpuAdjustData(d.data, A);
    g.putImageData(d, 0, 0);
  }
  return out;
}

export function draw(geo, adj, plain) {
  const A = plain ? ZERO : adj,
    sw = geo[0] === 0;
  const w = sw ? th : tw,
    h = sw ? tw : th;
  if (canvas.width !== w || canvas.height !== h) {
    canvas.width = w;
    canvas.height = h;
  }
  if (gl) {
    gl.viewport(0, 0, w, h);
    gl.uniformMatrix2fv(glU.g, false, new Float32Array(geo));
    gl.uniform1f(glU.e, A.exposure / 100);
    gl.uniform1f(glU.c, A.contrast / 100);
    gl.uniform1f(glU.a, A.saturation / 100);
    gl.uniform1f(glU.k, A.temperature / 100);
    gl.uniform1f(glU.h, A.highlights / 100);
    gl.uniform1f(glU.d, A.shadows / 100);
    gl.drawArrays(gl.TRIANGLE_STRIP, 0, 4);
    return;
  }
  if (!cpuSrc) return;
  cpu = cpu || canvas.getContext('2d', { willReadFrequently: true });
  cpu.setTransform(geo[0], geo[2], geo[1], geo[3], w / 2, h / 2);
  cpu.clearRect(-Math.max(w, h), -Math.max(w, h), 2 * Math.max(w, h), 2 * Math.max(w, h));
  cpu.drawImage(cpuSrc, -tw / 2, -th / 2);
  cpu.setTransform(1, 0, 0, 1, 0, 0);
  if (isZero(A) && !cpuLut) return;
  const d = cpu.getImageData(0, 0, w, h);
  cpuAdjustData(d.data, A);
  if (cpuLut) {
    const p = d.data;
    for (let i = 0; i < p.length; i += 4) {
      const at =
        (Math.round((p[i + 2] / 255) * 32) * 1089
          + Math.round((p[i + 1] / 255) * 32) * 33
          + Math.round((p[i] / 255) * 32))
        * 4;
      p[i] = cpuLut[at];
      p[i + 1] = cpuLut[at + 1];
      p[i + 2] = cpuLut[at + 2];
    }
  }
  cpu.putImageData(d, 0, 0);
}
