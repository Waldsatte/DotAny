// the stage: one model on the family's page, soft light, a soft shadow. It only draws when something has changed
import * as THREE from 'three';
import { OrbitControls } from 'three/examples/jsm/controls/OrbitControls.js';
import { RoomEnvironment } from 'three/examples/jsm/environments/RoomEnvironment.js';
import { mac, wheelSource, pinch } from '@dotany/web/gestures.js';

const bez = (t) => {
  // the family's curve, --e-out
  const x1 = 0.2,
    y1 = 0.7,
    x2 = 0.2,
    y2 = 1,
    X = (s) => 3 * x1 * s * (1 - s) ** 2 + 3 * x2 * s * s * (1 - s) + s ** 3,
    Y = (s) => 3 * y1 * s * (1 - s) ** 2 + 3 * y2 * s * s * (1 - s) + s ** 3;
  let a = 0,
    b = 1;
  for (let i = 0; i < 18; i++) {
    const m = (a + b) / 2;
    if (X(m) < t) a = m;
    else b = m;
  }
  return Y((a + b) / 2);
};
const VIEWS = {
  iso: [Math.PI * 0.22, Math.PI * 0.36],
  front: [0, Math.PI / 2],
  side: [Math.PI / 2, Math.PI / 2],
  top: [0, 0.0001],
};
const cssVar = (n) => getComputedStyle(document.documentElement).getPropertyValue(n).trim();
function cssColor(n, fallback) {
  const c = new THREE.Color(fallback);
  const v = cssVar(n);
  try {
    if (v && /^#|^rgb/.test(v)) c.set(v);
  } catch {
    /* keep the fallback */
  }
  return c;
}

function makeRenderer() {
  const r = new THREE.WebGLRenderer({ antialias: true, alpha: true, powerPreference: 'high-performance' });
  r.setPixelRatio(Math.min(devicePixelRatio || 1, 2));
  r.setClearColor(0, 0);
  r.autoClear = false;
  r.toneMapping = THREE.NeutralToneMapping;
  r.toneMappingExposure = 1;
  r.localClippingEnabled = true;
  r.shadowMap.enabled = true;
  r.shadowMap.type = THREE.PCFSoftShadowMap;
  return r;
}
// Before any model is open: the drawing context, and the soft room light's shaders compiled in the background (in parallel,
// KHR_parallel_shader_compile). Compiling them is most of the time making the stage takes, and done this way nothing
// waits for it, while the stage's own memory still waits for a model. It uses PMREMGenerator's inner parts; if three.js
// changes them, the stage simply compiles them itself when it's made.
export function prepare() {
  const renderer = makeRenderer(),
    pm = new THREE.PMREMGenerator(renderer),
    room = new RoomEnvironment();
  let ready = Promise.resolve();
  try {
    // the same programs pm.fromScene(room) asks for: the room and its backdrop drawn into the PMREM's target untoned, then the blur
    pm._setSize(256);
    const target = pm._allocateTargets();
    const all = new THREE.Scene(),
      plain = (m) => new THREE.Mesh(pm._lodMeshes[0].geometry, m);
    all.add(
      room,
      new THREE.Mesh(
        new THREE.BoxGeometry(),
        new THREE.MeshBasicMaterial({
          name: 'PMREM.Background',
          side: THREE.BackSide,
          depthWrite: false,
          depthTest: false,
        })
      )
    );
    [pm._blurMaterial, pm._ggxMaterial].filter(Boolean).forEach((m) => all.add(plain(m)));
    const tm = renderer.toneMapping;
    renderer.toneMapping = THREE.NoToneMapping;
    renderer.setRenderTarget(target);
    try {
      ready = renderer.compileAsync(all, new THREE.PerspectiveCamera(90, 1, 0.1, 100)).catch(() => {});
    } finally {
      renderer.setRenderTarget(null);
      renderer.toneMapping = tm;
      all.remove(room);
      target.dispose();
    }
  } catch {
    /* a newer three.js: the stage compiles them itself when it's made */
  }
  return { renderer, pm, room, ready };
}

// The Mac's trackpad (gestures.js): a trackpad has no wheel and no middle button, so its two fingers turn the model as a
// drag does, ⇧ with two fingers moves it, and the pinch zooms about the fingers (the pointer, for zoomToCursor). A mouse
// keeps the controls' own: its wheel zooms. The trackpad's scroll is taken before the controls' own listener on the
// canvas hears it.
function trackpad(c, host) {
  const fromTrackpad = wheelSource(),
    started = () => c.dispatchEvent({ type: 'start' });
  host.addEventListener(
    'wheel',
    (e) => {
      if (!c.enabled || e.ctrlKey || e.metaKey || fromTrackpad(e) !== 'trackpad') return;
      e.preventDefault();
      e.stopPropagation();
      started();
      // the fingers' way, as a drag would go (the deltas are the content's way, against the fingers)
      const h = c.domElement.clientHeight || 1;
      if (e.shiftKey) c.pan(-e.deltaX * c.panSpeed, -e.deltaY * c.panSpeed);
      else {
        c.rotateLeft(((2 * Math.PI * -e.deltaX) / h) * c.rotateSpeed);
        c.rotateUp(((2 * Math.PI * -e.deltaY) / h) * c.rotateSpeed);
      }
    },
    { capture: true, passive: false }
  );
  pinch(host, {
    begin: started,
    zoom: (k, x, y) => {
      if (!c.enabled) return;
      c._updateZoomParameters?.(x, y);
      // the fingers apart (k > 1) bring the model nearer: the controls' "dolly out" divides the distance
      c.dollyOut(k);
    },
  });
}

export class Viewer {
  constructor(host, reduce, prepared) {
    this.host = host;
    this.reduce = reduce;
    this.onFrame = null;
    this.onUserMove = null;
    const r = (this.renderer = prepared ? prepared.renderer : makeRenderer());
    host.appendChild(r.domElement);
    r.domElement.className = 'm-canvas';
    this.scene = new THREE.Scene();
    const pm = prepared ? prepared.pm : new THREE.PMREMGenerator(r);
    this.scene.environment = pm.fromScene(prepared ? prepared.room : new RoomEnvironment(), 0.04).texture;
    pm.dispose();
    this.scene.environmentIntensity = 0.85;
    this.camera = new THREE.PerspectiveCamera(30, 1, 0.01, 1000);
    this.camera.layers.enable(1);
    this.sun = new THREE.DirectionalLight(0xffffff, 1.4);
    this.sun.castShadow = true;
    this.sun.shadow.mapSize.set(2048, 2048);
    this.sun.shadow.radius = 9;
    this.sun.shadow.blurSamples = 16;
    this.sun.layers.enableAll();
    this.scene.add(this.sun, this.sun.target);
    // what the model sits in. Rotating this is how a Z-up file stands up, without touching the file
    this.holder = new THREE.Group();
    this.scene.add(this.holder);
    this.ground = new THREE.Mesh(new THREE.PlaneGeometry(1, 1), new THREE.ShadowMaterial({ opacity: 0.16 }));
    this.ground.rotation.x = -Math.PI / 2;
    this.ground.receiveShadow = true;
    this.ground.layers.set(1);
    this.scene.add(this.ground);
    this.helpers = new THREE.Group();
    this.helpers.traverse((o) => o.layers.set(1));
    this.scene.add(this.helpers);
    this.controls = new OrbitControls(this.camera, r.domElement);
    const c = this.controls;
    c.enableDamping = true;
    c.dampingFactor = 0.09;
    c.zoomToCursor = true;
    c.rotateSpeed = 0.8;
    c.panSpeed = 0.9;
    c.mouseButtons = { LEFT: THREE.MOUSE.ROTATE, MIDDLE: THREE.MOUSE.PAN, RIGHT: THREE.MOUSE.PAN };
    c.addEventListener('change', () => {
      this.dirty = true;
    });
    c.addEventListener('start', () => {
      this.tween = null;
      if (this.onUserMove) this.onUserMove();
    });
    if (mac) trackpad(c, host);
    this.mode = 'original';
    this.dirty = true;
    this.clip = null;
    this.mixer = null;
    this.playing = false;
    this.section = null;
    this.padR = 0;
    this.padNow = 0;
    this.padFrom = 0;
    this.padAt = -1;
    this.clay = new THREE.MeshStandardMaterial({ color: 0xb9b6ae, roughness: 0.62, metalness: 0 });
    this.fill = new THREE.MeshBasicMaterial({
      color: 0xffffff,
      polygonOffset: true,
      polygonOffsetFactor: 1,
      polygonOffsetUnits: 1,
    });
    this.wire = new THREE.MeshBasicMaterial({ color: 0x151413, wireframe: true, transparent: true, opacity: 0.62 });
    this.syncTheme();
    this.ro = new ResizeObserver(() => this.resize());
    this.ro.observe(host);
    this.resize();
    this.clock = new THREE.Clock();
    addEventListener('pagehide', () => {
      cancelAnimationFrame(this.raf);
      r.dispose();
      r.forceContextLoss();
    });
    const loop = () => {
      this.raf = requestAnimationFrame(loop);
      this.tick();
    };
    loop();
  }
  syncTheme() {
    this.fill.color.copy(cssColor('--bg', '#ECEBE7'));
    this.wire.color.copy(cssColor('--text', '#151413'));
    const dark = this.fill.color.getHSL({}).l < 0.4;
    this.ground.material.opacity = dark ? 0.42 : 0.16;
    this.syncInk();
    this.dirty = true;
  }
  // lines and points without colours of their own (a G-code path, a scan's cloud: models.js) take the page's ink
  syncInk() {
    if (this.root)
      this.root.traverse((o) => {
        if ((o.isLine || o.isPoints) && o.material.userData.ink && !o.material.vertexColors)
          o.material.color.copy(this.wire.color);
      });
  }
  resize() {
    const w = this.host.clientWidth || 1,
      h = this.host.clientHeight || 1;
    this.renderer.setSize(w, h);
    this.camera.aspect = w / h;
    this.applyOffset();
    this.dirty = true;
  }
  // the model centres in the room the panel and the bars leave it
  applyOffset() {
    const w = this.host.clientWidth || 1,
      h = this.host.clientHeight || 1;
    this.camera.setViewOffset(w, h, this.padNow / 2, (this.padB || 0) / 2, w, h);
    this.camera.updateProjectionMatrix();
  }
  setPad(right, bottomExtra = 0) {
    this.rise = bottomExtra;
    this.padFrom = this.padNow;
    this.padR = right;
    this.padB = 21 + bottomExtra / 2;
    this.padAt = this.reduce ? -1 : performance.now();
    if (this.reduce) {
      this.padNow = right;
      this.applyOffset();
    }
    this.dirty = true;
  }
  tick() {
    const now = performance.now();
    if (this.padAt >= 0) {
      const t = Math.min(1, (now - this.padAt) / 420);
      this.padNow = this.padFrom + (this.padR - this.padFrom) * bez(t);
      this.applyOffset();
      this.dirty = true;
      if (t >= 1) this.padAt = -1;
    }
    if (this.tween) {
      const q = this.tween,
        t = Math.min(1, (now - q.at) / q.ms),
        e = bez(t);
      this.camera.position.lerpVectors(q.p0, q.p1, e);
      this.controls.target.lerpVectors(q.t0, q.t1, e);
      this.camera.position.copy(q.centre(e));
      this.camera.lookAt(this.controls.target);
      this.dirty = true;
      if (t >= 1) {
        this.tween = null;
        this.controls.update();
      }
    } else if (this.root) this.controls.update();
    const dt = this.clock.getDelta();
    if (this.mixer && this.playing) {
      this.mixer.update(dt);
      this.dirty = true;
      if (this.onTime) this.onTime();
    }
    if (this.dirty) this.draw();
  }
  draw() {
    this.dirty = false;
    const r = this.renderer;
    r.clear();
    if (this.mode === 'wire') {
      this.scene.overrideMaterial = this.fill;
      this.camera.layers.set(0);
      r.render(this.scene, this.camera);
      this.scene.overrideMaterial = this.wire;
      r.render(this.scene, this.camera);
    } else {
      this.scene.overrideMaterial = this.mode === 'clay' ? this.clay : null;
      this.camera.layers.set(0);
      r.render(this.scene, this.camera);
    }
    this.scene.overrideMaterial = null;
    this.camera.layers.set(1);
    r.shadowMap.autoUpdate = false;
    r.render(this.scene, this.camera);
    r.shadowMap.autoUpdate = true;
    this.camera.layers.enable(0);
    this.camera.layers.enable(1);
    if (this.onFrame) this.onFrame();
  }
  // something about to change all at once fades over itself: last picture on top, going away
  crossfade(change, host = this.host) {
    if (this.reduce || !this.root) {
      change();
      this.dirty = true;
      return;
    }
    this.draw();
    const src = this.renderer.domElement,
      c = document.createElement('canvas');
    c.width = src.width;
    c.height = src.height;
    c.className = 'm-fade';
    c.getContext('2d').drawImage(src, 0, 0);
    host.appendChild(c);
    change();
    this.draw();
    const a = c.animate([{ opacity: 1 }, { opacity: 0 }], { duration: 240, easing: 'cubic-bezier(.2,.7,.2,1)' });
    a.onfinish = a.oncancel = () => c.remove();
  }
  setModel(root, clips, zUp) {
    this.clear();
    this.root = root;
    this.clips = clips || [];
    this.holder.rotation.x = zUp ? -Math.PI / 2 : 0;
    this.holder.add(root);
    this.holder.updateMatrixWorld(true);
    root.traverse((o) => {
      if (o.isMesh) {
        o.castShadow = true;
        o.layers.set(0);
      }
    });
    this.syncInk();
    this.mixer = this.clips.length ? new THREE.AnimationMixer(root) : null;
    this.action = null;
    this.playing = false;
    if (this.clips.length) this.pickClip(0);
    this.place();
    this.setView('iso', true);
  }
  clear() {
    if (this.root) {
      this.holder.remove(this.root);
      this.root.traverse((o) => {
        if (o.isMesh || o.isLine || o.isPoints) {
          o.geometry.dispose();
          [].concat(o.material).forEach((m) => {
            for (const k in m) if (m[k] && m[k].isTexture) m[k].dispose();
            m.dispose();
          });
        }
      });
    }
    this.root = null;
    this.section = null;
    this.helpers.clear();
    this.mixer = null;
    this.dirty = true;
  }
  setUp(zUp) {
    this.crossfade(() => {
      this.holder.rotation.x = zUp ? -Math.PI / 2 : 0;
      this.holder.updateMatrixWorld(true);
      this.place();
      this.setView(this.view || 'iso', true);
    });
  }
  // the size a person reads off the info panel is the shape itself, point by point, not the boxes of its rotated parts
  visibleBox() {
    const b = new THREE.Box3();
    this.holder.updateMatrixWorld(true);
    this.root.traverse((o) => {
      if (o.isMesh && this.shown(o)) b.expandByObject(o, true);
    });
    if (b.isEmpty()) b.setFromObject(this.root);
    return b;
  }
  shown(o) {
    for (let q = o; q; q = q.parent) if (!q.visible) return false;
    return true;
  }
  // the ground goes under it, the sun above and to the side, all in proportion to it
  place() {
    const b = (this.box = this.visibleBox()),
      s = b.getSize(new THREE.Vector3()),
      c = (this.center = b.getCenter(new THREE.Vector3())),
      r = (this.radius = Math.max(s.length() / 2, 1e-6));
    this.ground.position.set(c.x, b.min.y - r * 0.004, c.z);
    this.ground.scale.setScalar(r * 14);
    this.sun.position.set(c.x + r * 1.1, c.y + r * 3.2, c.z + r * 1.5);
    this.sun.target.position.copy(c);
    this.sun.target.updateMatrixWorld();
    const cam = this.sun.shadow.camera;
    cam.left = cam.bottom = -r * 2.4;
    cam.right = cam.top = r * 2.4;
    cam.near = r * 0.1;
    cam.far = r * 8;
    cam.updateProjectionMatrix();
    this.sun.shadow.bias = -0.0004;
    this.sun.shadow.normalBias = r * 0.004;
    this.sun.shadow.needsUpdate = true;
    this.camera.near = r / 200;
    this.camera.far = r * 60;
    this.camera.updateProjectionMatrix();
    this.controls.minDistance = r * 0.05;
    this.controls.maxDistance = r * 16;
    this.dirty = true;
  }
  distanceFor(box) {
    const s = box.getSize(new THREE.Vector3()),
      r = Math.max(s.length() / 2, 1e-6),
      w = this.host.clientWidth - this.padR,
      h = this.host.clientHeight - 53 - 96 - (this.rise || 0),
      vfov = (this.camera.fov * Math.PI) / 360,
      fit = Math.min(1, w / h / Math.max(this.camera.aspect, 0.1));
    return (((r / Math.sin(vfov)) * Math.max(1, this.host.clientHeight / h)) / Math.max(fit, 0.5)) * 0.74;
  }
  // far enough that every corner of the box stays inside the clear part of the window, whatever the shape
  distanceToClear(box, dir) {
    const H = this.host.clientHeight || 1,
      W = this.host.clientWidth || 1,
      h = Math.max(H - 53 - 96 - (this.rise || 0), 1),
      w = Math.max(W - this.padR, 1),
      t = Math.tan((this.camera.fov * Math.PI) / 360),
      c = box.getCenter(new THREE.Vector3()),
      right = new THREE.Vector3(0, 1, 0).cross(dir);
    if (right.lengthSq() < 1e-8) right.set(1, 0, 0);
    right.normalize();
    const up = dir.clone().cross(right).normalize();
    let d = 0;
    for (const x of [box.min.x, box.max.x])
      for (const y of [box.min.y, box.max.y])
        for (const z of [box.min.z, box.max.z]) {
          const v = new THREE.Vector3(x, y, z).sub(c),
            toward = v.dot(dir);
          d = Math.max(
            d,
            toward + Math.abs(v.dot(up)) / ((t * h) / H),
            toward + Math.abs(v.dot(right)) / ((t * this.camera.aspect * w) / W)
          );
        }
    return d * 1.04;
  }
  setView(name, jump, box) {
    this.view = name;
    const [az, pol] = VIEWS[name] || VIEWS.iso,
      tgt = (box || this.box).getCenter(new THREE.Vector3()),
      dir = new THREE.Vector3().setFromSphericalCoords(1, pol, az),
      d = Math.max(this.distanceFor(box || this.box), this.distanceToClear(box || this.box, dir)),
      off = dir.clone().multiplyScalar(d),
      p1 = tgt.clone().add(off);
    if (jump || this.reduce) {
      this.tween = null;
      this.camera.position.copy(p1);
      this.controls.target.copy(tgt);
      this.controls.update();
      this.dirty = true;
      return;
    }
    // around the target, not through the model
    const t0 = this.controls.target.clone(),
      s0 = new THREE.Spherical().setFromVector3(this.camera.position.clone().sub(t0)),
      s1 = new THREE.Spherical(d, pol, az);
    let dth = s1.theta - s0.theta;
    dth = ((((dth + Math.PI) % (Math.PI * 2)) + Math.PI * 2) % (Math.PI * 2)) - Math.PI;
    const tmp = new THREE.Spherical(),
      v = new THREE.Vector3();
    this.tween = {
      at: performance.now(),
      ms: 420,
      p0: this.camera.position.clone(),
      p1,
      t0,
      t1: tgt,
      centre: (e) => {
        tmp.set(s0.radius + (s1.radius - s0.radius) * e, s0.phi + (s1.phi - s0.phi) * e, s0.theta + dth * e);
        v.setFromSpherical(tmp);
        return v.add(this.controls.target);
      },
    };
  }
  setMode(m) {
    if (m === this.mode) return;
    this.crossfade(() => {
      this.mode = m;
      this.ground.visible = m !== 'wire';
    });
  }
  // ---------- animation ----------
  pickClip(i) {
    if (!this.mixer) return;
    if (this.action) this.action.stop();
    this.clip = this.clips[i];
    this.action = this.mixer.clipAction(this.clip);
    this.action.play();
    this.action.paused = false;
    this.mixer.setTime(0);
    this.dirty = true;
  }
  seek(t) {
    if (this.mixer) {
      this.mixer.setTime(t);
      this.dirty = true;
    }
  }
  time() {
    return this.action ? this.action.time : 0;
  }
  // ---------- cutting through ----------
  setSection(sec) {
    const was = this.section;
    this.section = sec;
    if (!this.root) return;
    this.root.traverse((o) => {
      if (!o.isMesh) return;
      [].concat(o.material).forEach((m) => {
        if (sec) {
          if (m.userData.side == null) m.userData.side = m.side;
          m.side = THREE.DoubleSide;
          m.clippingPlanes = [sec];
          m.clipShadows = true;
        } else if (m.userData.side != null) {
          m.side = m.userData.side;
          m.clippingPlanes = null;
        }
        m.needsUpdate = true;
      });
    });
    [this.clay, this.fill, this.wire].forEach((m) => {
      m.clippingPlanes = sec ? [sec] : null;
      m.clipShadows = true;
      m.side = sec ? THREE.DoubleSide : THREE.FrontSide;
      m.needsUpdate = true;
    });
    void was;
    this.dirty = true;
  }
  // ---------- hits ----------
  hit(x, y) {
    if (!this.root) return null;
    const r = this.renderer.domElement.getBoundingClientRect(),
      nd = new THREE.Vector2(((x - r.left) / r.width) * 2 - 1, -((y - r.top) / r.height) * 2 + 1),
      ray = (this.ray = this.ray || new THREE.Raycaster());
    ray.setFromCamera(nd, this.camera);
    ray.layers.set(0);
    const list = ray
      .intersectObject(this.root, true)
      .filter(
        (h) => h.object.isMesh && this.shown(h.object) && (!this.section || this.section.distanceToPoint(h.point) >= 0)
      );
    return list[0] || null;
  }
  project(p) {
    const v = p.clone().project(this.camera),
      r = this.host.getBoundingClientRect();
    return [((v.x + 1) / 2) * r.width, ((1 - v.y) / 2) * r.height, v.z];
  }
  // ---------- a picture of it ----------
  async snapshot(height, transparent) {
    const r = this.renderer,
      w0 = this.host.clientWidth,
      h0 = this.host.clientHeight,
      pr = r.getPixelRatio(),
      helpersOn = this.helpers.visible,
      w = Math.round((height * w0) / h0),
      bg = this.fill.color;
    this.helpers.visible = false;
    r.setPixelRatio(1);
    r.setSize(w, height, false);
    this.camera.clearViewOffset();
    this.camera.aspect = w / height;
    this.camera.updateProjectionMatrix();
    if (!transparent) {
      r.setClearColor(bg, 1);
    }
    this.draw();
    const blob = await new Promise((res) => r.domElement.toBlob(res, 'image/png'));
    r.setClearColor(0, 0);
    this.helpers.visible = helpersOn;
    r.setPixelRatio(pr);
    this.resize();
    this.draw();
    return { blob, w, h: height };
  }
}
export { THREE };
