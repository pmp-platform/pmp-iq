// minichart — a tiny self-contained SVG chart renderer (vendored, no CDN; M35).
// Exposes window.MiniChart with line, histogram, scatter, treemap and sparkline.
// Refresh: this file is hand-maintained (no build step); edit in place.
(function (global) {
  "use strict";

  var NS = "http://www.w3.org/2000/svg";
  function el(name, attrs) {
    var node = document.createElementNS(NS, name);
    for (var k in attrs) { if (attrs[k] != null) { node.setAttribute(k, attrs[k]); } }
    return node;
  }
  function svg(w, h) { return el("svg", { viewBox: "0 0 " + w + " " + h, width: "100%", height: h, style: "overflow:visible" }); }
  function extent(arr) {
    var lo = Infinity, hi = -Infinity;
    arr.forEach(function (v) { if (v < lo) lo = v; if (v > hi) hi = v; });
    if (lo === Infinity) { lo = 0; hi = 1; }
    if (lo === hi) { hi = lo + 1; }
    return [lo, hi];
  }
  function color(t) {
    // red (0) → green (1).
    var r = Math.round(220 * (1 - t)) + 20, g = Math.round(180 * t) + 40;
    return "rgb(" + r + "," + g + ",70)";
  }
  function mount(target, node) {
    var c = typeof target === "string" ? document.querySelector(target) : target;
    if (!c) { return; }
    c.innerHTML = "";
    c.appendChild(node);
  }
  function empty(target, msg) {
    var c = typeof target === "string" ? document.querySelector(target) : target;
    if (c) { c.innerHTML = '<div class="text-xs text-slate-400">' + (msg || "No data yet.") + "</div>"; }
  }

  // points: [{day, value}]
  function line(target, points, opts) {
    opts = opts || {};
    if (!points || points.length === 0) { return empty(target); }
    var w = opts.width || 480, h = opts.height || 160, pad = 24;
    var ys = points.map(function (p) { return p.value; });
    var ye = extent(ys);
    var node = svg(w, h);
    function x(i) { return pad + (w - 2 * pad) * (points.length === 1 ? 0.5 : i / (points.length - 1)); }
    function y(v) { return h - pad - (h - 2 * pad) * (v - ye[0]) / (ye[1] - ye[0]); }
    node.appendChild(el("line", { x1: pad, y1: h - pad, x2: w - pad, y2: h - pad, stroke: "#cbd5e1" }));
    var d = points.map(function (p, i) { return (i ? "L" : "M") + x(i) + " " + y(p.value); }).join(" ");
    node.appendChild(el("path", { d: d, fill: "none", stroke: opts.stroke || "#2563eb", "stroke-width": 2 }));
    points.forEach(function (p, i) {
      var dot = el("circle", { cx: x(i), cy: y(p.value), r: 2.5, fill: "#2563eb" });
      var title = el("title", {}); title.textContent = p.day + ": " + p.value.toFixed(2);
      dot.appendChild(title); node.appendChild(dot);
    });
    mount(target, node);
  }

  // buckets: [{lo, hi, count}]
  function histogram(target, buckets, opts) {
    opts = opts || {};
    if (!buckets || buckets.length === 0) { return empty(target); }
    var w = opts.width || 480, h = opts.height || 160, pad = 24;
    var maxC = extent(buckets.map(function (b) { return b.count; }))[1];
    var bw = (w - 2 * pad) / buckets.length;
    var node = svg(w, h);
    buckets.forEach(function (b, i) {
      var bh = (h - 2 * pad) * (b.count / maxC);
      var rect = el("rect", { x: pad + i * bw + 1, y: h - pad - bh, width: bw - 2, height: bh, fill: opts.fill || "#0ea5e9" });
      var title = el("title", {}); title.textContent = b.lo.toFixed(1) + "–" + b.hi.toFixed(1) + ": " + b.count;
      rect.appendChild(title); node.appendChild(rect);
    });
    node.appendChild(el("line", { x1: pad, y1: h - pad, x2: w - pad, y2: h - pad, stroke: "#cbd5e1" }));
    mount(target, node);
  }

  // items: [{x, y, size, label, href}]
  function scatter(target, items, opts) {
    opts = opts || {};
    if (!items || items.length === 0) { return empty(target); }
    var w = opts.width || 480, h = opts.height || 240, pad = 30;
    var xe = extent(items.map(function (d) { return d.x; }));
    var ye = extent(items.map(function (d) { return d.y; }));
    var se = extent(items.map(function (d) { return d.size || 1; }));
    var node = svg(w, h);
    node.appendChild(el("line", { x1: pad, y1: h - pad, x2: w - pad, y2: h - pad, stroke: "#cbd5e1" }));
    node.appendChild(el("line", { x1: pad, y1: pad, x2: pad, y2: h - pad, stroke: "#cbd5e1" }));
    items.forEach(function (d) {
      var cx = pad + (w - 2 * pad) * (d.x - xe[0]) / (xe[1] - xe[0]);
      var cy = h - pad - (h - 2 * pad) * (d.y - ye[0]) / (ye[1] - ye[0]);
      var r = 3 + 9 * ((d.size || 1) - se[0]) / (se[1] - se[0]);
      var dot = el("circle", { cx: cx, cy: cy, r: r, fill: "#6366f1", "fill-opacity": 0.6, style: d.href ? "cursor:pointer" : "" });
      if (d.href) { dot.addEventListener("click", function () { location.href = d.href; }); }
      var title = el("title", {}); title.textContent = (d.label || "") + " (" + d.x + ", " + d.y + ")";
      dot.appendChild(title); node.appendChild(dot);
    });
    mount(target, node);
  }

  // tiles: [{size, value, label, href}] — value in [0,1] maps red→green.
  function treemap(target, tiles, opts) {
    opts = opts || {};
    if (!tiles || tiles.length === 0) { return empty(target); }
    var w = opts.width || 480, h = opts.height || 240;
    var total = tiles.reduce(function (s, t) { return s + (t.size || 1); }, 0);
    // Simple row-based squarify approximation.
    var node = svg(w, h), x = 0, y = 0, rowH = h / Math.ceil(Math.sqrt(tiles.length));
    var perRow = Math.ceil(w / (w / Math.ceil(Math.sqrt(tiles.length))));
    tiles.forEach(function (t, i) {
      var tw = Math.max(20, (w * (t.size || 1)) / total * (Math.sqrt(tiles.length)));
      if (x + tw > w) { x = 0; y += rowH; }
      var rect = el("rect", { x: x + 1, y: y + 1, width: Math.min(tw, w - x) - 2, height: rowH - 2, fill: color(t.value == null ? 0.5 : t.value), style: t.href ? "cursor:pointer" : "" });
      if (t.href) { rect.addEventListener("click", function () { location.href = t.href; }); }
      var title = el("title", {}); title.textContent = (t.label || "") + (t.value != null ? " — " + Math.round(t.value * 100) + "%" : "");
      rect.appendChild(title); node.appendChild(rect);
      x += Math.min(tw, w - x);
    });
    void perRow;
    mount(target, node);
  }

  // values: [number] — compact inline trend line.
  function sparkline(target, values, opts) {
    opts = opts || {};
    if (!values || values.length === 0) { return empty(target, "—"); }
    var w = opts.width || 90, h = opts.height || 22;
    var e = extent(values);
    var node = svg(w, h);
    var d = values.map(function (v, i) {
      var x = (w - 2) * (values.length === 1 ? 0.5 : i / (values.length - 1)) + 1;
      var y = h - 2 - (h - 4) * (v - e[0]) / (e[1] - e[0]);
      return (i ? "L" : "M") + x.toFixed(1) + " " + y.toFixed(1);
    }).join(" ");
    node.appendChild(el("path", { d: d, fill: "none", stroke: opts.stroke || "#2563eb", "stroke-width": 1.5 }));
    mount(target, node);
  }

  global.MiniChart = { line: line, histogram: histogram, scatter: scatter, treemap: treemap, sparkline: sparkline };
})(window);
