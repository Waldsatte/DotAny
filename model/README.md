# DotAny 3D

DotAny 3D opens a 3D model and lets you look at it: turn it, measure it, cut through it, play its animation, take its
parts apart, read its facts, and save it in an open format or as a picture. The file is left alone until you save. It is
a [Tauri 2](https://tauri.app) program: a page in `src/` (plain JavaScript and [three.js](https://threejs.org), built
with esbuild) and a Rust program in `src-tauri/`. It runs on Windows (on Microsoft Edge WebView2) and on the Mac. The
folder is `model/`, the crate `dotany-model` and the program `dotany-model.exe`; the name people see is DotAny 3D.

## What it opens and saves

| Format | Extensions | Opens | Can be saved as |
|---|---|---|---|
| glTF binary, glTF | `.glb`, `.gltf` | yes, with its `.bin` files and pictures beside it | GLB, OBJ, STL |
| Wavefront OBJ | `.obj` | yes, with its `.mtl` and pictures beside it | GLB, OBJ, STL |
| STL | `.stl` | yes, binary or text | GLB, OBJ, STL |
| 3MF | `.3mf` | yes, read only | STL first, also GLB, OBJ |
| FBX | `.fbx` | yes, read only | GLB first, also OBJ, STL |
| PLY | `.ply` | yes, read only (a mesh, or only points) | GLB first, also OBJ, STL |
| OFF | `.off` | yes, read only | OBJ first, also GLB, STL |
| G-code | `.gcode`, `.gco`, `.nc` | yes, read only (a path, not a solid) | GLB, OBJ |
| MagicaVoxel | `.vox` | yes, read only (versions 150 and 200) | GLB first, also OBJ, STL |

A picture of the stage can be saved as a PNG from the same card, at 1080, 2160 or 4320 pixels high, with or without a
transparent background; it is not a model, so it does not go on the recent list. A path or a cloud of points has no
faces, so STL is not offered for it.

A glTF that requires an extension this program does not read (compressed textures such as KTX2 or Basis, for example) is
refused with a notice, as is an FBX of a version the reader does not support. Draco and meshopt compression are read.
Nothing is ever written compressed.

## What you can do

Turn the model with the left button, move it with the middle or right button, and zoom with the wheel towards the
pointer. Jump to a perspective, front, side or top view; double-click a part to bring it into view. Show it with its
materials, as plain clay or as a wireframe. Measure (`M`): its three edges are measured, and two clicks measure the
distance between two points, snapping to a nearby corner. Cut through it (`C`): choose an axis, slide the plane, flip its
side. Play its animations (`Space`), scrub the time, pick a clip. Open the parts list to select, hide or isolate a part
and to show them all again. Open the facts panel: format, bounding size, triangles, vertices, materials, textures,
animations, file size and, for formats such as STL, OBJ and 3MF, whether it is closed and its volume. A file that does not say what
its numbers mean can be set to millimetres, centimetres, metres or inches, and a model that stands on Z instead of Y can
be stood up. Saving writes GLB, OBJ or STL, leaving out the hidden parts unless you switch that off.

## Building

From this folder, with Node.js and Rust stable (on Windows, the MSVC toolchain):

```powershell
npm ci
npx tauri build --no-bundle
```

This builds the page (`node scripts/build.mjs`, which writes `dist/`) and then the program, `dotany-model.exe` in
`release/` under Cargo's target folder (`CARGO_TARGET_DIR` if set, otherwise `src-tauri/target`). The page build is the
family's, `shared/scripts/build.mjs`, with 3D's own additions in `scripts/build.mjs`: Draco's decoder from three.js is
packed into `src/draco-bin.js` (generated, not committed), and the licences of the libraries inside three.js are added
to the notices. `src/icons.js` is written by the page build from `lucide-static`, and `src/notices.generated.js`, the
licence notices the program shows, by `shared/scripts/notices.mjs`. The Mac's application and its `Info.plist` and
`lproj/` folders are made by `shared/scripts/mac-release.mjs` and `shared/scripts/mac-plist.mjs`.

## Where things are

**The program** (`src-tauri/src/app.rs`): what makes 3D itself (`TOOL`), the commands the page calls (`read_model`,
`adopt_model`, `have_beside`, `save_model`), and the `dotmodel` address that serves the files a model names beside
itself. It reads no format: that is the page's work.

**Shared with the rest of DotAny:** `dotany-base` (`../shared/rust`): the window, atomic saves, settings, recent files,
the watcher that notices when another program changes the file, fonts and the platform layer.

**The page** (`src/`):

- `main.js`: the window, the dock, the measuring marks, the section cut, the parts and facts panels, opening and saving.
- `viewer.js`: the stage: the three.js scene, the soft light, the camera and its controls, the display modes, the
  section plane, hit tests and the picture of the stage.
- `models.js`: reading each format into a three.js scene, the facts about it (`statsOf`, `solidOf`, `voxSolid`), and
  writing GLB, OBJ and STL. `draco.js`: the Draco decoder, loaded only for a model that uses it.
- `backend.js`: the calls to the Rust side. `lang/`: 3D's words in each language, added to the family's.
- From `shared/web` (`@dotany/web/…`): the window's frame and top bar, menus, settings and the Save As card, keys,
  gestures, sound, and the family's words and licences. `shared/web/family.css` comes first in the built stylesheet;
  `src/styles.css` only adds to it.

## How it works

**The page reads the model.** `read_model` hands the page the file's bytes whole. The page picks the three.js loader
for the extension (`readModel` in `models.js`) and answers a scene, its animation clips, the format's name, whether it
stands on Z, the unit its numbers are in and whether that unit is fixed by the format. Only once the page has read it
does it call `adopt_model`, which makes it the window's open file, watched and first in the recent list.

**Files beside the model.** A glTF names its buffers and pictures, an OBJ its materials and a material its textures. The
page asks which of them exist (`have_beside`) and the loaders fetch the rest from `dotmodel:`, an address the program
serves from the folder of the window's model and from nowhere else. A missing buffer stops the open, with the name of
the file that is needed; a missing picture is drawn as a plain colour and named in a notice.

**The stage** draws only when something has changed. The model sits in a holder that is turned when the file stands on
Z, so the file itself is never touched; the ground, the sun's shadow and the camera's distance are worked out from the
model's size, which is the size of what is shown, so a hidden part does not count. The soft light is three.js's room
environment; its shaders are compiled in the background once the window is up (`prepare` in `viewer.js`), and three.js,
the stage and the readers are loaded after the empty window is ready rather than with it, so the first model does not
wait for them. Lines and points (a G-code path, a scan's cloud) take the page's ink and follow the theme.

**The facts.** Triangles and vertices are counted. A solid is welded by position within a millionth of its size, and
every edge is counted: an edge in one triangle is open, in more than two is non-manifold, and a shape with neither is
closed. Its volume is the sum of signed tetrahedra. This is skipped above four million triangles. A voxel model is
checked from its voxels, since its mesh joins faces. G-code is read as a machine reads it: `G0` to `G3` (arcs cut into
short pieces), absolute and relative moves, inches and millimetres, and the extruder; moves that push filament are shown,
and the moves between them only when nothing is printed.

**Writing.** Each format expects its own frame: glTF stands Y up in metres, STL Z up in millimetres, OBJ Y up and read
here in centimetres. A model is turned and scaled on its way out and opens again at the same size and the same way up.
Saving a file in its own format with the same parts left out writes the original bytes. Bytes the page made are written
whole by `save_model`, through an atomic write.

## What it is careful about

- **Only the model's folder.** A name a file gives for something beside it is looked for inside the model's folder. A
  name that reaches outside it, or an absolute path from the computer it was made on, is looked for by its last part
  only, and a file is served only if its real path is inside the folder.
- **Decoding only.** Draco and meshopt are decoders; nothing is written compressed. Draco's decoder is packed into the
  program and run in a worker from a `blob:`, which is why the content security policy allows `wasm-unsafe-eval` and
  `blob:` workers and nothing from the network.
- **Large models.** A model of more than 1.5 million triangles gets plain vertex normals instead of creased ones. A
  G-code file past 60 million numbers, or an OFF file with more than 50 million points or faces, is refused. An FBX waits
  at most 15 seconds for its textures. A glTF's morph targets do not count towards its size.
- **Sizes.** A model's size is the shape itself, point by point, not the boxes of its rotated parts, and only what is
  shown counts.
- **Units.** A file's numbers mean what its unit says. glTF is in metres, 3MF names its unit and G-code is in
  millimetres; an OBJ or an FBX is taken as centimetres, and an STL, PLY, OFF or VOX as millimetres (a voxel is one
  unit), until the unit is set in the facts panel.

## Windows and the Mac

- **Windows.** Each model opens in a program of its own, with one window. The window has no system title bar: the page
  draws the top bar and its buttons. `windows-app-manifest.xml` declares Windows 10 and 11 support, which Windows
  requires before it shows the snap layouts over the maximise button. The program serves companion files from
  `http://dotmodel.localhost/`.
- **The Mac.** One program shows each model in a window of its own, each drawn by its own page, and each window serves
  only its own model's folder. The address is `dotmodel://localhost/`, which the page asks Tauri for. On a trackpad two
  fingers turn the model, `Shift` with two fingers moves it, and a pinch zooms about the fingers; a mouse keeps the wheel
  zoom. WebKit has no `requestIdleCallback`, so the background loading waits a moment instead.
