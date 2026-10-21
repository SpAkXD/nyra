// ---- local images of the live views (src/web/images.js, added by src/web.rs when a view may show one)
// The build copied every image a view may show under a hashed name (`img/photo-1a2b3c4d.jpg`), with
// its WebP variants and blurred preview, also one that only a click brings. `ny_imgs` (written by
// the build before this) maps the name the program wrote to [file, srcset, width, height, preview
// class]. A view's fresh HTML gets what the build gave the image on the static page, so a patch
// changes nothing that has loaded. The files are relative to the page, as the script's own `src`.
const ny_up = (() => {
    const s = typeof document !== "undefined" && document.currentScript;
    const a = (s && s.getAttribute("src")) || "";
    return a.slice(0, a.lastIndexOf("/") + 1);
})();
const NY_REFS = ["src", "href", "poster", "data-src", "data-full", "data-bg", "data-image"];
function ny_img_fix(root) {
    for (const e of root.querySelectorAll("*")) {
        const img = e.nodeName === "IMG";
        const first = e.getAttribute("fetchpriority") === "high";
        for (const a of NY_REFS) {
            const v = e.getAttribute(a);
            const i = v !== null && Object.prototype.hasOwnProperty.call(ny_imgs, v) ? ny_imgs[v] : null;
            if (!i || (a === "href" && e.nodeName !== "A" && e.nodeName !== "LINK")) continue;
            e.setAttribute(a, ny_up + i[0]);
            if (!img || a !== "src") continue;
            if (i[2] && !e.hasAttribute("width") && !e.hasAttribute("height")) {
                e.setAttribute("width", i[2]);
                e.setAttribute("height", i[3]);
            }
            if (i[1] && !e.hasAttribute("srcset")) {
                e.setAttribute("srcset", i[1].split(", ").map((x) => ny_up + x).join(", "));
                if (!e.hasAttribute("sizes")) e.setAttribute("sizes", "100vw");
            }
            if (i[4] && !first) ny_class(e, [i[4]]);
        }
        if (img && !e.hasAttribute("loading") && !first) e.setAttribute("loading", "lazy");
        if (img && !e.hasAttribute("decoding")) e.setAttribute("decoding", "async");
    }
}
