// Downloads the official FUJIFILM product thumbnails for every camera that supports
// USB RAW conversion (the X RAW STUDIO compatibility list) into src/assets/cameras/.
//
// The images are © FUJIFILM. They are NOT committed (see .gitignore) and are for
// personal use only — run `npm run fetch-cameras` once after cloning.
// Missing files are fine: the app falls back to a generic camera icon.
//
// Keys are the model names the camera reports in its PTP DeviceInfo, lower-cased with
// spaces/dashes removed (see cameraImage() in src/components/CameraImage.tsx).

import { mkdir, writeFile, access } from "node:fs/promises";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const CDN = "https://fujifilm-x.b-cdn.net/wp-content/uploads/";

// source: product pages on fujifilm-x.com (2026-11-18)
const IMAGES = {
  gfx100ii: "2023/09/wfqs_gfx100-ii_thum.jpg",
  gfx100: "2018/09/gfx100_thum.jpg",
  gfx100sii: "2024/05/gfx_100s_ii_small.jpg",
  gfx100s: "2021/01/gfx100s_thum_egsv.jpg",
  gfx50sii: "2021/09/rghs_GFX50SII_thumbnail.jpg",
  gfx50s: "2017/08/1_thum_gfx_50s.jpg",
  gfx50r: "2018/09/0_thum_gfx50r.jpg",
  gfx100rf: "2025/03/product-list_gfx100rf.jpg",
  xh2s: "2022/05/sgew_x-h2s_thum-1.png",
  xh2: "2022/09/ajgz_x-h2_thum.png",
  xh1: "2018/06/1_thum_x-h1.jpg",
  xpro3: "2019/10/xpro3_thum.jpg",
  xpro2: "2016/10/2_thum_x-pro2.jpg",
  xt5: "2022/10/koes_x-t5_thum.jpg",
  xt4: "2020/02/x-t4_thum.jpg",
  xt2: "2018/09/thum_x-t2.jpg",
  xs20: "2023/05/dwga_x-s20_thum.jpg",
  xs10: "2020/10/x-s10_product_list-poiu.jpg",
  xt50: "2024/05/product-list_X-T50_front_16-50_silver.jpg",
  xt30iii: "2025/10/thum_x-t30-iii-004c6e21.webp",
  xt30ii: "2021/09/x-t30-ii_thum_bxyg.jpg",
  xt20: "2017/01/5_thum_x-t20.jpg",
  xe5: "2025/06/thum_X-E5-1.jpg",
  xe4: "2021/01/x-e4_thumbnail_gkac.jpg",
  xe3: "2017/09/7_thum_x-e3.jpg",
  xm5: "2024/10/product_thumb_x-m5.jpg",
  x100vi: "2024/02/stjw_x100vi_thum.jpg",
  x100v: "2020/02/x100v_thum.jpg",
  x100f: "2017/01/1_thum_x100f.jpg",
};

const out = join(dirname(fileURLToPath(import.meta.url)), "..", "src", "assets", "cameras");
await mkdir(out, { recursive: true });

let ok = 0;
let failed = 0;
for (const [key, path] of Object.entries(IMAGES)) {
  const ext = path.slice(path.lastIndexOf("."));
  const file = join(out, key + ext);
  try {
    await access(file);
    ok++;
    continue; // already there
  } catch {}
  try {
    const r = await fetch(CDN + path, { headers: { "User-Agent": "Mozilla/5.0" } });
    if (!r.ok) throw new Error(`HTTP ${r.status}`);
    await writeFile(file, Buffer.from(await r.arrayBuffer()));
    console.log("ok  ", key);
    ok++;
  } catch (e) {
    console.log("FAIL", key, String(e));
    failed++;
  }
}
console.log(`${ok} ok, ${failed} failed -> ${out}`);
