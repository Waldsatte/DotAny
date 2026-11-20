// Reading the formats, the facts a person wants to know, and writing the open formats. The files a model names beside
// itself come from its own folder, through the program's dotmodel address.
import * as THREE from 'three';
import { GLTFLoader } from 'three/examples/jsm/loaders/GLTFLoader.js';
import { OBJLoader } from 'three/examples/jsm/loaders/OBJLoader.js';
import { MTLLoader } from 'three/examples/jsm/loaders/MTLLoader.js';
import { STLLoader } from 'three/examples/jsm/loaders/STLLoader.js';
import { ThreeMFLoader } from 'three/examples/jsm/loaders/3MFLoader.js';
import { FBXLoader } from 'three/examples/jsm/loaders/FBXLoader.js';
import { PLYLoader } from 'three/examples/jsm/loaders/PLYLoader.js';
import { VOXLoader, buildMesh as voxMesh } from 'three/examples/jsm/loaders/VOXLoader.js';
import { GLTFExporter } from 'three/examples/jsm/exporters/GLTFExporter.js';
import { OBJExporter } from 'three/examples/jsm/exporters/OBJExporter.js';
import { STLExporter } from 'three/examples/jsm/exporters/STLExporter.js';
import { toCreasedNormals } from 'three/examples/jsm/utils/BufferGeometryUtils.js';
import { unzipSync, strFromU8 } from 'three/examples/jsm/libs/fflate.module.js';

export class Compressed extends Error {}
export class OldFbx extends Error {}
export class Missing extends Error {
  constructor(names) {
    super('missing');
    this.names = names;
  }
}

// where the files beside the model are served from (src-tauri/src/app.rs). Tauri states a scheme of the program's own
// differently on each system (http://dotmodel.localhost/ on Windows, dotmodel://localhost/ on the Mac), so it is asked;
// outside the program (the readers' unit test) nothing is fetched and Windows' form stands in
const BASE = globalThis.window?.__TAURI_INTERNALS__?.convertFileSrc('', 'dotmodel') ?? 'http://dotmodel.localhost/';
// a picture that isn't there shows the plain colour, not black, and isn't an error
const BLANK =
  'data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mP8/x8AAwMCAO+ip1sAAAAASUVORK5CYII=';
const lastPart = (n) => n.split(/[\\/]/).pop();
const unescape = (n) => {
  try {
    return decodeURIComponent(n);
  } catch {
    return n;
  }
};

const SOFT_LIMIT = 1500000;
function softNormals(geometry) {
  const tris = (geometry.index ? geometry.index.count : geometry.attributes.position.count) / 3;
  if (tris > SOFT_LIMIT) {
    geometry.computeVertexNormals();
    return geometry;
  }
  return toCreasedNormals(geometry, Math.PI / 6);
}
const plain = () => new THREE.MeshStandardMaterial({ color: 0xc8c5bd, roughness: 0.55, metalness: 0.02 });
// lines and points (a G-code path, a scan's cloud) are drawn in the page's ink: the stage keeps it in step with the theme
// (viewer.js, syncTheme)
const ink = (m) => {
  m.userData.ink = true;
  return m;
};

// every name a file gives for something beside it goes to the model's folder; one that isn't there becomes a blank
function managerFor(gone, missing) {
  const m = new THREE.LoadingManager();
  m.setURLModifier((url) => {
    if (/^(data|blob):/.test(url)) return url;
    const name = unescape(url.startsWith(BASE) ? url.slice(BASE.length) : url);
    if (gone.has(name)) {
      missing.add(lastPart(name));
      return BLANK;
    }
    return BASE + encodeURIComponent(name);
  });
  m.onError = (url) => missing.add(lastPart(unescape(url.startsWith(BASE) ? url.slice(BASE.length) : url)));
  return m;
}

function gltfJson(bytes, ext) {
  try {
    let text;
    if (ext === 'gltf') text = new TextDecoder().decode(bytes);
    else {
      const dv = new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength);
      if (dv.getUint32(0, true) !== 0x46546c67) return null;
      const n = dv.getUint32(12, true);
      text = new TextDecoder().decode(bytes.subarray(20, 20 + n));
    }
    return JSON.parse(text);
  } catch {
    return null;
  }
}
const external = (list) =>
  (list || [])
    .map((x) => x.uri)
    .filter((u) => u && !/^data:/.test(u))
    .map(unescape);

/**
 * bytes: the whole file. name: its name. have(names): which of these are beside it, from the program.
 * Answers { root, clips, kind, format, zUp, unit, fixedUnit, missing }; throws Compressed, OldFbx, Missing, or anything
 * else for a file that is damaged or isn't a model.
 */
export async function readModel(bytes, name, have = async (n) => n.map(() => true)) {
  const ext = name.split('.').pop().toLowerCase(),
    missing = new Set(),
    gone = new Set();
  const check = async (names, fatal) => {
    const there = names.length ? await have(names) : [];
    const lost = names.filter((_, i) => !there[i]);
    if (fatal && lost.length) throw new Missing(lost.map(lastPart));
    lost.forEach((n) => gone.add(n));
  };
  const buffer = () => bytes.buffer.slice(bytes.byteOffset, bytes.byteOffset + bytes.byteLength);

  if (ext === 'glb' || ext === 'gltf') {
    const j = gltfJson(bytes, ext);
    if (!j) throw new Error('bad');
    // what it needs that isn't supported is said first, before what's missing beside it
    const used = j.extensionsUsed || [],
      required = j.extensionsRequired || [];
    const known =
      /^(KHR_(materials_\w+|texture_transform|lights_punctual|mesh_quantization|draco_mesh_compression|meshopt_compression)|EXT_(meshopt_compression|mesh_gpu_instancing|texture_webp))$/;
    if (required.some((x) => !known.test(x))) throw new Compressed();
    await check(external(j.buffers), true);
    await check(external(j.images), false);
    const loader = new GLTFLoader(managerFor(gone, missing));
    let draco = null;
    if (used.includes('KHR_draco_mesh_compression')) {
      const { PackedDraco } = await import('./draco.js');
      draco = new PackedDraco();
      loader.setDRACOLoader(draco);
    }
    if (used.some((x) => /meshopt_compression$/.test(x))) {
      const { MeshoptDecoder } = await import('three/examples/jsm/libs/meshopt_decoder.module.js');
      loader.setMeshoptDecoder(MeshoptDecoder);
    }
    const gltf = await new Promise((res, rej) => loader.parse(buffer(), '', res, rej))
      .catch((e) => {
        if (/KTX|BasisU|basisu|draco worker/i.test(String(e && e.message))) throw new Compressed();
        throw e;
      })
      .finally(() => {
        if (draco) draco.dispose();
      });
    let any = 0;
    gltf.scene.traverse((o) => {
      if (o.isMesh) any++;
    });
    // a path or a cloud saved here from G-code or PLY is lines or points, and opens again
    if (!any)
      gltf.scene.traverse((o) => {
        if (o.isLine || o.isPoints) {
          any++;
          o.material = ink(
            new (o.isPoints ? THREE.PointsMaterial : THREE.LineBasicMaterial)({
              size: 2,
              sizeAttenuation: false,
              vertexColors: !!o.geometry.attributes.color,
            })
          );
        }
      });
    if (!any) throw new Error('empty');
    restingBoxes(gltf.scene);
    return {
      root: gltf.scene,
      clips: gltf.animations || [],
      kind: 'glb',
      format: ext.toUpperCase(),
      zUp: false,
      unit: 'm',
      fixedUnit: true,
      missing: [...missing],
    };
  }

  // FBX is read and never written: saving one goes to the open formats, so its kind is GLB
  if (ext === 'fbx') {
    const manager = managerFor(gone, missing);
    let began = false;
    manager.onStart = () => {
      began = true;
    };
    const settled = new Promise((res) => {
      manager.onLoad = res;
    });
    let root;
    try {
      root = new FBXLoader(manager).parse(buffer(), '');
    } catch (e) {
      if (/FBX version not supported/i.test(String(e && e.message))) throw new OldFbx();
      throw e;
    }
    if (began) await Promise.race([settled, new Promise((res) => setTimeout(res, 15000))]);
    // the stage has its own light and view, and a picture that couldn't be read leaves the plain colour, not black
    const aside = [];
    let any = 0;
    root.traverse((o) => {
      if (o.isLight || o.isCamera) aside.push(o);
      if (!o.isMesh) return;
      any++;
      oneRunPerMaterial(o.geometry);
      [].concat(o.material).forEach((m) => {
        for (const k in m)
          if (m[k] && m[k].isTexture && !m[k].image) {
            m[k] = null;
            m.needsUpdate = true;
          }
      });
    });
    aside.forEach((o) => o.removeFromParent());
    if (!any) throw new Error('empty');
    restingBoxes(root);
    return {
      root,
      clips: root.animations || [],
      kind: 'glb',
      format: 'FBX',
      zUp: false,
      unit: 'cm',
      fixedUnit: false,
      missing: [...missing],
    };
  }

  if (ext === 'obj') {
    const text = new TextDecoder().decode(bytes),
      manager = managerFor(gone, missing),
      loader = new OBJLoader(manager),
      libs = [...text.matchAll(/^mtllib\s+(.+?)\s*$/gm)].map((m) => m[1]);
    let withMtl = false;
    if (libs.length) {
      const there = await have(libs),
        lib = libs.find((_, i) => there[i]);
      libs.filter((_, i) => !there[i]).forEach((n) => missing.add(lastPart(n)));
      if (lib) {
        const mtl = await (await fetch(BASE + encodeURIComponent(lib))).text();
        await check(
          [
            ...new Set(
              [...mtl.matchAll(/^\s*(?:map_\w+|bump|disp|decal|refl)\s+(?:-\S+\s+\S+\s+)*(.+?)\s*$/gim)].map(
                (m) => m[1]
              )
            ),
          ],
          false
        );
        const mats = new MTLLoader(manager).parse(mtl, '');
        mats.preload();
        loader.setMaterials(mats);
        withMtl = true;
      }
    }
    const root = loader.parse(text);
    let any = 0;
    root.traverse((o) => {
      if ((o.isLine || o.isPoints) && o.geometry.attributes.position.count) {
        any++;
        o.material = ink(
          new (o.isPoints ? THREE.PointsMaterial : THREE.LineBasicMaterial)({ size: 2, sizeAttenuation: false })
        );
      }
    });
    root.traverse((o) => {
      if (!o.isMesh) return;
      const pos = o.geometry.attributes.position;
      if (!pos || !pos.count) return;
      any++;
      if (!o.geometry.attributes.normal || !withMtl) o.geometry = softNormals(o.geometry);
      if (!withMtl) o.material = [].concat(o.material).length > 1 ? [].concat(o.material).map(plain) : plain();
      else
        [].concat(o.material).forEach((q) => {
          q.side = THREE.DoubleSide;
        });
    });
    if (!any) throw new Error('empty');
    return {
      root,
      clips: [],
      kind: 'obj',
      format: 'OBJ',
      zUp: false,
      unit: 'cm',
      fixedUnit: false,
      missing: [...missing],
    };
  }

  if (ext === 'stl') {
    const g = new STLLoader().parse(buffer());
    if (!g.attributes.position || !g.attributes.position.count) throw new Error('empty');
    const mesh = new THREE.Mesh(softNormals(g), plain());
    if (g.hasColors) mesh.material.vertexColors = true;
    mesh.name = name.replace(/\.[^.]+$/, '');
    const root = new THREE.Group();
    root.add(mesh);
    return { root, clips: [], kind: 'stl', format: 'STL', zUp: true, unit: 'mm', fixedUnit: false };
  }

  // 3MF: millimetres unless the file says otherwise, Z up, read only (it's saved as the open formats)
  if (ext === '3mf') {
    const parts = unzipSync(new Uint8Array(buffer()), { filter: (f) => /\.model$/i.test(f.name) }),
      first = Object.values(parts)[0];
    if (!first) throw new Error('bad');
    const unitWord =
      (/<model\b[^>]*\bunit\s*=\s*"(\w+)"/i.exec(strFromU8(first.subarray(0, 4096))) || [])[1] || 'millimeter';
    const root = new ThreeMFLoader(managerFor(gone, missing)).parse(buffer());
    let any = 0;
    root.traverse((o) => {
      if (!o.isMesh) return;
      any++;
      if (!o.geometry.attributes.normal) o.geometry = softNormals(o.geometry);
      [].concat(o.material).forEach((q) => {
        if (q.isMeshPhongMaterial || q.isMeshBasicMaterial) q.side = THREE.DoubleSide;
      });
    });
    if (!any) throw new Error('empty');
    const UNIT = { micron: 'um', millimeter: 'mm', centimeter: 'cm', meter: 'm', inch: 'in', foot: 'ft' };
    return {
      root,
      clips: [],
      kind: '3mf',
      format: '3MF',
      zUp: true,
      unit: UNIT[unitWord.toLowerCase()] || 'mm',
      fixedUnit: true,
      missing: [...missing],
    };
  }
  // ---------- the formats that are only read: saved as GLB, OBJ or STL ----------
  // PLY: a scan's mesh, or only its points. Its units aren't said: millimetres, as for STL, until the person picks
  if (ext === 'ply') {
    let g = new PLYLoader().parse(buffer());
    const pos = g.attributes.position;
    if (!pos || !pos.count) throw new Error('empty');
    const colours = !!g.attributes.color,
      root = new THREE.Group();
    let part,
      cloud = false;
    if (g.index && g.index.count) {
      if (!g.attributes.normal) g = softNormals(g);
      part = new THREE.Mesh(g, plain());
      if (colours) part.material.vertexColors = true;
    } else {
      cloud = true;
      part = new THREE.Points(
        g,
        ink(new THREE.PointsMaterial({ size: 2, sizeAttenuation: false, vertexColors: colours }))
      );
    }
    part.name = name.replace(/\.[^.]+$/, '');
    root.add(part);
    return { root, clips: [], kind: 'ply', format: 'PLY', zUp: false, unit: 'mm', fixedUnit: false, solidless: cloud };
  }
  // OFF: Geomview's text of points and faces (any number of corners, as fans), with colours on the faces if it has them
  if (ext === 'off') {
    const g = readOff(new TextDecoder().decode(bytes));
    const mesh = new THREE.Mesh(g.attributes.color ? g : softNormals(g), plain());
    if (g.attributes.color) {
      g.computeVertexNormals();
      mesh.material.vertexColors = true;
    }
    mesh.name = name.replace(/\.[^.]+$/, '');
    const root = new THREE.Group();
    root.add(mesh);
    return { root, clips: [], kind: 'off', format: 'OFF', zUp: false, unit: 'mm', fixedUnit: false };
  }
  // G-code: a printer's or a mill's path, in millimetres, Z up. What it prints is shown; the moves between, only when it
  // prints nothing (a mill's path). Not a solid: no faces, no volume
  if (ext === 'gcode' || ext === 'gco' || ext === 'nc') {
    const { printed, moves } = readGcode(new TextDecoder().decode(bytes));
    const keep = printed.length ? printed : moves;
    if (!keep.length) throw new Error('empty');
    const g = new THREE.BufferGeometry();
    g.setAttribute('position', new THREE.BufferAttribute(new Float32Array(keep), 3));
    const path = new THREE.LineSegments(g, ink(new THREE.LineBasicMaterial())),
      root = new THREE.Group();
    path.name = name.replace(/\.[^.]+$/, '');
    root.add(path);
    return {
      root,
      clips: [],
      kind: 'gcode',
      format: 'G-code',
      zUp: true,
      unit: 'mm',
      fixedUnit: true,
      solidless: true,
    };
  }
  // VOX: MagicaVoxel's voxels, each model's cubes as one mesh in its palette's colours. One voxel is one unit
  if (ext === 'vox') {
    // the loader reads versions 150 and 200; for another it only prints to the console, so that is refused here
    const version = bytes.byteLength >= 8 ? new DataView(buffer()).getUint32(4, true) : 0;
    if (version !== 150 && version !== 200) throw new Error('bad');
    const r = new VOXLoader().parse(buffer());
    if (!r || !r.chunks || !r.chunks.length) throw new Error('bad');
    let root = r.scene;
    if (!root) {
      root = new THREE.Group();
      r.chunks.forEach((c) => root.add(voxMesh(c)));
    } else if (!root.isGroup) {
      const g = new THREE.Group();
      g.add(root);
      root = g;
    }
    let any = 0;
    root.traverse((o) => {
      if (o.isMesh && o.geometry.attributes.position.count) any++;
    });
    if (!any) throw new Error('empty');
    // closed and volume from the voxels themselves: the mesh joins faces into larger ones, and the edges where a large
    // face meets two small ones would read as open. Only when each model is shown once
    const solid = any === r.chunks.length ? voxSolid(r.chunks) : null;
    return {
      root,
      clips: [],
      kind: 'vox',
      format: 'VOX',
      zUp: false,
      unit: 'mm',
      fixedUnit: false,
      solid,
      solidless: !solid,
    };
  }
  throw new Error('format');
}

// A voxel model is always closed, except where two voxels meet only along an edge (with both cells beside that edge
// empty): that edge belongs to four faces. Its volume is its voxels, one unit each
export function voxSolid(chunks) {
  let volume = 0,
    nonManifold = 0;
  for (const c of chunks) {
    const { x: sx, y: sy, z: sz } = c.size,
      d = c.data,
      filled = new Uint8Array(sx * sy * sz);
    const at = (x, y, z) =>
      x < 0 || y < 0 || z < 0 || x >= sx || y >= sy || z >= sz ? 0 : filled[x + y * sx + z * sx * sy];
    for (let i = 0; i + 3 < d.length; i += 4)
      if (d[i] < sx && d[i + 1] < sy && d[i + 2] < sz && !filled[d[i] + d[i + 1] * sx + d[i + 2] * sx * sy]) {
        filled[d[i] + d[i + 1] * sx + d[i + 2] * sx * sy] = 1;
        volume++;
      }
    // each pair of axes, each voxel with a neighbour across the diagonal of that plane
    const axes = [
      [
        [1, 0, 0],
        [0, 1, 0],
      ],
      [
        [0, 1, 0],
        [0, 0, 1],
      ],
      [
        [1, 0, 0],
        [0, 0, 1],
      ],
    ];
    for (let z = 0; z < sz; z++)
      for (let y = 0; y < sy; y++)
        for (let x = 0; x < sx; x++) {
          if (!filled[x + y * sx + z * sx * sy]) continue;
          for (const [a, b] of axes)
            for (const k of [1, -1]) {
              const ax = a[0],
                ay = a[1],
                az = a[2],
                bx = b[0] * k,
                by = b[1] * k,
                bz = b[2] * k;
              if (
                at(x + ax + bx, y + ay + by, z + az + bz)
                && !at(x + ax, y + ay, z + az)
                && !at(x + bx, y + by, z + bz)
              )
                nonManifold++;
            }
        }
  }
  return { open: 0, nonManifold, closed: nonManifold === 0, volume };
}

// G-code, as printers and mills read it: G0 and G1 move in a straight line, G2 and G3 along an arc (I, J from the start;
// cut into short pieces), G90 and G91 absolute or relative, G20 and G21 inches or millimetres, G92 sets where it is,
// M82 and M83 make only the extruder absolute or relative. A move that pushes filament (E grows) is printed; every other
// move goes between. Line numbers (N), comments (; and parentheses) and several words on a line are read as machines do.
// Answers both lists of segments, in millimetres, as flat x y z pairs
export function readGcode(text) {
  const printed = [],
    moves = [];
  let x = 0,
    y = 0,
    z = 0,
    e = 0,
    rel = false,
    relE = null,
    scale = 1,
    any = false,
    modal = 'G0';
  for (const raw of text.split(/\r?\n/)) {
    const line = raw.replace(/\([^)]*\)|;.*$/g, '').toUpperCase();
    const words = [...line.matchAll(/([A-Z])\s*([-+]?(?:\d+\.?\d*|\.\d+))/g)];
    if (!words.length) continue;
    const arg = {},
      codes = [];
    for (const [, k, v] of words) {
      if (k === 'G' || k === 'M') codes.push(k + Number(v));
      else arg[k] = Number(v);
    }
    for (const c of codes) {
      if (c === 'G90') {
        rel = false;
        relE = null;
      } else if (c === 'G91') {
        rel = true;
        relE = null;
      } else if (c === 'M82') relE = false;
      else if (c === 'M83') relE = true;
      else if (c === 'G20') scale = 25.4;
      else if (c === 'G21') scale = 1;
      else if (c === 'G92') {
        if ('X' in arg) x = arg.X * scale;
        if ('Y' in arg) y = arg.Y * scale;
        if ('Z' in arg) z = arg.Z * scale;
        if ('E' in arg) e = arg.E;
      }
    }
    // a line of only coordinates goes on as the last G0 to G3 did; setting the place, homing and M words don't move
    const g = codes.find((c) => /^G[0-3]$/.test(c));
    if (g) modal = g;
    const move = !g && codes.some((c) => c === 'G92' || c === 'G28' || c[0] === 'M') ? null : g || modal;
    if (!move || !('X' in arg || 'Y' in arg || 'Z' in arg)) {
      if ('E' in arg && move) e = (relE ?? rel) ? e + arg.E : arg.E;
      continue;
    }
    const to = (k, v) => (k in arg ? (rel ? v + arg[k] * scale : arg[k] * scale) : v);
    const nx = to('X', x),
      ny = to('Y', y),
      nz = to('Z', z);
    let pushes = false;
    if ('E' in arg) {
      const ne = (relE ?? rel) ? e + arg.E : arg.E;
      pushes = ne > e + 1e-9;
      e = ne;
    }
    const list = pushes ? printed : moves;
    if (move === 'G2' || move === 'G3') {
      // around the centre at (I, J) from the start, the short way the code says; Z goes evenly along
      const cx = x + (arg.I || 0) * scale,
        cy = y + (arg.J || 0) * scale,
        r = Math.hypot(x - cx, y - cy);
      let a0 = Math.atan2(y - cy, x - cx),
        a1 = Math.atan2(ny - cy, nx - cx);
      if (move === 'G2' && a1 >= a0) a1 -= 2 * Math.PI;
      if (move === 'G3' && a1 <= a0) a1 += 2 * Math.PI;
      const n = Math.max(2, Math.ceil((Math.abs(a1 - a0) * Math.max(r, 1)) / 0.5));
      let px = x,
        py = y,
        pz = z;
      for (let i = 1; i <= n; i++) {
        const t = i / n,
          a = a0 + (a1 - a0) * t,
          qx = i === n ? nx : cx + r * Math.cos(a),
          qy = i === n ? ny : cy + r * Math.sin(a),
          qz = z + (nz - z) * t;
        list.push(px, py, pz, qx, qy, qz);
        px = qx;
        py = qy;
        pz = qz;
      }
    } else if (nx !== x || ny !== y || nz !== z) list.push(x, y, z, nx, ny, nz);
    x = nx;
    y = ny;
    z = nz;
    any = true;
    if (printed.length + moves.length > 6e7) throw new Error('too large');
  }
  if (!any) throw new Error('bad');
  return { printed, moves };
}

// OFF, from the Geomview manual: an optional header word ([ST][C][N]OFF), the counts (points, faces, edges), one point
// to a line (what follows x y z on it, a normal or a colour, isn't needed), then one face to a line: its number of
// corners, their indices, and an optional colour (0..1, or 0..255 as integers). "#" starts a comment. The binary and the
// 4D forms aren't read
export function readOff(text) {
  const lines = text
    .split(/\r?\n/)
    .map((l) => l.replace(/#.*/, '').trim())
    .filter(Boolean)
    .map((l) => l.split(/\s+/));
  let li = 0,
    wi = 0;
  const next = () => {
    while (li < lines.length && wi >= lines[li].length) {
      li++;
      wi = 0;
    }
    if (li >= lines.length) throw new Error('bad');
    return lines[li][wi++];
  };
  const num = () => {
    const v = Number(next());
    if (!Number.isFinite(v)) throw new Error('bad');
    return v;
  };
  const endLine = () => {
    if (wi > 0) {
      li++;
      wi = 0;
    }
  };
  const head = /^(ST)?(C)?(N)?(4)?(n)?OFF$/.exec((lines[0] || [])[0] || '');
  if (head) {
    if (head[4] || head[5]) throw new Error('bad');
    next();
    if ((lines[li] || [])[wi] === 'BINARY') throw new Error('bad');
  }
  const nv = num(),
    nf = num();
  num();
  endLine();
  if (!(nv > 0 && nf > 0) || nv > 5e7 || nf > 5e7) throw new Error('bad');
  const pts = new Float32Array(nv * 3);
  for (let i = 0; i < nv; i++) {
    pts[i * 3] = num();
    pts[i * 3 + 1] = num();
    pts[i * 3 + 2] = num();
    endLine();
  }
  const tris = [],
    cols = [];
  let coloured = false;
  for (let f = 0; f < nf; f++) {
    const n = num(),
      ids = [];
    for (let k = 0; k < n; k++) {
      const i = num();
      if (i < 0 || i >= nv || !Number.isInteger(i)) throw new Error('bad');
      ids.push(i);
    }
    // what's left on the face's line is its colour
    const rest = wi > 0 ? lines[li].slice(wi, wi + 3).map(Number) : [];
    endLine();
    let c = null;
    if (rest.length === 3 && rest.every(Number.isFinite)) {
      const big = rest.some((v) => v > 1);
      c = rest.map((v) => (big ? v / 255 : v));
      coloured = true;
    }
    for (let k = 1; k + 1 < n; k++) {
      tris.push(ids[0], ids[k], ids[k + 1]);
      cols.push(c, c, c);
    }
  }
  if (!tris.length) throw new Error('empty');
  const g = new THREE.BufferGeometry();
  if (!coloured) {
    g.setAttribute('position', new THREE.BufferAttribute(pts, 3));
    g.setIndex(tris);
    return g;
  }
  // colours belong to faces: every corner its own point
  const p = new Float32Array(tris.length * 3),
    col = new Float32Array(tris.length * 3),
    grey = [0.78, 0.77, 0.74];
  tris.forEach((i, k) => {
    p.set(pts.subarray(i * 3, i * 3 + 3), k * 3);
    col.set(cols[k] || grey, k * 3);
  });
  g.setAttribute('position', new THREE.BufferAttribute(p, 3));
  g.setAttribute('color', new THREE.BufferAttribute(col, 3));
  return g;
}

// a shape's size is the shape at rest, not every shape its morph targets could become
function restingBoxes(root) {
  root.traverse((o) => {
    if (o.isMesh && !o.isSkinnedMesh && Object.keys(o.geometry.morphAttributes).length)
      o.geometry.boundingBox = new THREE.Box3().setFromBufferAttribute(o.geometry.attributes.position);
  });
}
// FBX keeps a run of triangles for every change of material; one run per material draws and saves far lighter, and nothing visible moves
function oneRunPerMaterial(g) {
  if (g.index || g.groups.length < 2 || new Set(g.groups.map((x) => x.materialIndex)).size === g.groups.length) return;
  const order = [...g.groups].sort((a, b) => a.materialIndex - b.materialIndex || a.start - b.start),
    remap = (a) => {
      const n = a.itemSize,
        out = new a.array.constructor(a.array.length);
      let w = 0;
      for (const r of order) {
        const from = r.start * n,
          len = r.count * n;
        out.set(a.array.subarray(from, from + len), w);
        w += len;
      }
      return new THREE.BufferAttribute(out, n, a.normalized);
    };
  for (const k of Object.keys(g.attributes)) g.setAttribute(k, remap(g.attributes[k]));
  for (const k of Object.keys(g.morphAttributes)) g.morphAttributes[k] = g.morphAttributes[k].map(remap);
  const runs = [];
  let at = 0;
  for (const r of order) {
    const last = runs[runs.length - 1];
    if (last && last.materialIndex === r.materialIndex) last.count += r.count;
    else runs.push({ start: at, count: r.count, materialIndex: r.materialIndex });
    at += r.count;
  }
  g.clearGroups();
  runs.forEach((r) => g.addGroup(r.start, r.count, r.materialIndex));
}

// ---------- facts ----------
export function statsOf(root) {
  let tris = 0,
    verts = 0,
    meshes = 0;
  const mats = new Set(),
    tex = new Set();
  // a path's or a cloud's points are its vertices; it has no triangles
  root.traverse((o) => {
    if (o.isLine || o.isPoints) verts += o.geometry.attributes.position.count;
  });
  root.traverse((o) => {
    if (!o.isMesh) return;
    meshes++;
    const g = o.geometry,
      n = g.attributes.position.count;
    verts += n;
    tris += (g.index ? g.index.count : n) / 3;
    [].concat(o.material).forEach((m) => {
      mats.add(m);
      for (const k in m) if (m[k] && m[k].isTexture) tex.add(m[k]);
    });
  });
  return { tris: Math.round(tris), verts, meshes, materials: mats.size, textures: tex.size };
}

// welding by position, the way slicers do: points closer than `tol` are one point. A small hash table of its own keeps
// this quick for millions of points
function weld(pos, tol) {
  const n = pos.count,
    ids = new Int32Array(n);
  let size = 1;
  while (size < n * 2) size <<= 1;
  const slot = new Int32Array(size).fill(-1),
    qx = new Float64Array(n),
    qy = new Float64Array(n),
    qz = new Float64Array(n);
  let count = 0;
  for (let i = 0; i < n; i++) {
    const x = Math.round(pos.getX(i) / tol),
      y = Math.round(pos.getY(i) / tol),
      z = Math.round(pos.getZ(i) / tol);
    let h = (Math.imul(x | 0, 73856093) ^ Math.imul(y | 0, 19349663) ^ Math.imul(z | 0, 83492791)) & (size - 1);
    for (;;) {
      const s = slot[h];
      if (s < 0) {
        slot[h] = count;
        qx[count] = x;
        qy[count] = y;
        qz[count] = z;
        ids[i] = count++;
        break;
      }
      if (qx[s] === x && qy[s] === y && qz[s] === z) {
        ids[i] = s;
        break;
      }
      h = (h + 1) & (size - 1);
    }
  }
  return { ids, count };
}
// is it closed, and how much is inside: what a printer needs to hear. Every edge of a closed shape belongs to exactly
// two triangles; one is an open edge, three or more a non-manifold one
export function solidOf(root) {
  let open = 0,
    nonManifold = 0,
    volume = 0;
  const a = new THREE.Vector3(),
    b = new THREE.Vector3(),
    c = new THREE.Vector3();
  const box = new THREE.Box3().setFromObject(root),
    tol = Math.max(box.getSize(new THREE.Vector3()).length() * 1e-6, 1e-9);
  root.updateMatrixWorld(true);
  const parentInv = new THREE.Matrix4().copy(root.matrixWorld).invert(),
    m = new THREE.Matrix4();
  root.traverse((o) => {
    if (!o.isMesh) return;
    const g = o.geometry,
      pos = g.attributes.position,
      idx = g.index,
      n = idx ? idx.count : pos.count;
    if (!n) return;
    m.multiplyMatrices(parentInv, o.matrixWorld);
    const { ids, count: W } = weld(pos, tol),
      edges = new Float64Array(n);
    let e = 0;
    for (let t = 0; t + 2 < n; t += 3) {
      const i0 = idx ? idx.getX(t) : t,
        i1 = idx ? idx.getX(t + 1) : t + 1,
        i2 = idx ? idx.getX(t + 2) : t + 2,
        p = ids[i0],
        q = ids[i1],
        r = ids[i2];
      if (p !== q && q !== r && r !== p) {
        edges[e++] = p < q ? p * W + q : q * W + p;
        edges[e++] = q < r ? q * W + r : r * W + q;
        edges[e++] = r < p ? r * W + p : p * W + r;
      }
      a.fromBufferAttribute(pos, i0).applyMatrix4(m);
      b.fromBufferAttribute(pos, i1).applyMatrix4(m);
      c.fromBufferAttribute(pos, i2).applyMatrix4(m);
      volume += a.dot(b.cross(c)) / 6;
    }
    const sorted = edges.subarray(0, e).sort();
    for (let i = 0; i < e;) {
      let j = i + 1;
      while (j < e && sorted[j] === sorted[i]) j++;
      if (j - i === 1) open++;
      else if (j - i > 2) nonManifold++;
      i = j;
    }
  });
  return { open, nonManifold, closed: open === 0 && nonManifold === 0, volume: Math.abs(volume) };
}

// ---------- writing ----------
// What a format expects: glTF stands Y up in metres; STL stands Z up in millimetres; OBJ stands Y up, and DotAny reads it
// in centimetres. So a model is turned and scaled on its way out, and opens again the same size and the same way up.
export const UNITS = { um: 0.001, mm: 1, cm: 10, m: 1000, in: 25.4, ft: 304.8 };
const WANT = { glb: { mm: 1000, zUp: false }, obj: { mm: 10, zUp: false }, stl: { mm: 1, zUp: true } };
function outward(root, model, fmt, run) {
  const gone = [];
  root.traverse((o) => {
    if (!o.visible && o !== root) gone.push([o, o.parent]);
  });
  gone.forEach(([o, p]) => p.remove(o));
  const keep = root.parent,
    wrap = new THREE.Group(),
    w = WANT[fmt],
    k = UNITS[model.unit] / w.mm;
  if (keep) keep.remove(root);
  wrap.rotation.x = model.zUp === w.zUp ? 0 : model.zUp ? -Math.PI / 2 : Math.PI / 2;
  wrap.scale.setScalar(k);
  const bare = wrap.rotation.x === 0 && k === 1,
    top = bare ? root : wrap;
  if (!bare) wrap.add(root);
  top.updateMatrixWorld(true);
  const done = () => {
    if (!bare) wrap.remove(root);
    if (keep) keep.add(root);
    gone.forEach(([o, p]) => p.add(o));
    root.updateMatrixWorld(true);
  };
  return Promise.resolve()
    .then(() => run(top))
    .finally(done);
}
export const writeGlb = (root, model, clips) =>
  outward(
    root,
    model,
    'glb',
    async (top) =>
      new Uint8Array(
        await new GLTFExporter().parseAsync(top, { binary: true, animations: clips || [], onlyVisible: false })
      )
  );
export const writeObj = (root, model) =>
  outward(root, model, 'obj', (top) => new TextEncoder().encode(new OBJExporter().parse(top)));
export const writeStl = (root, model) =>
  outward(root, model, 'stl', (top) => {
    const dv = new STLExporter().parse(top, { binary: true });
    return new Uint8Array(dv.buffer, dv.byteOffset, dv.byteLength);
  });
