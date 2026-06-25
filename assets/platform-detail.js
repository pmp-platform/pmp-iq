// Generic platform entity detail renderer.
(function ($) {
  "use strict";

  var meta = JSON.parse($("#detail-meta").text());

  function esc(v) { return $("<div>").text(v === null || v === undefined ? "—" : v).html(); }

  function title(d) {
    return d.name || d.username || "Detail";
  }

  function field(label, value) {
    return '<div class="flex gap-2 text-sm"><span class="w-32 text-slate-500">' +
      label + "</span><span>" + esc(value) + "</span></div>";
  }

  function section(heading, rows, renderRow) {
    if (!rows || !rows.length) return "";
    var body = rows.map(renderRow).join("");
    return '<div class="bg-white rounded-lg shadow border border-slate-200 p-4">' +
      '<h2 class="font-semibold mb-2">' + heading + "</h2>" + body + "</div>";
  }

  function link(app) {
    return '<a class="text-blue-600" href="/platform/applications/' + app.id + '">' + esc(app.name) + "</a>";
  }

  function render(d) {
    $("#detail-title").text(title(d));
    var html = "";
    var base = '<div class="bg-white rounded-lg shadow border border-slate-200 p-4 space-y-1">';
    ["app_type", "primary_language", "description", "ecosystem", "kind", "version", "email"].forEach(function (k) {
      if (d[k] !== undefined && d[k] !== null) base += field(k, d[k]);
    });
    base += "</div>";
    html += base;

    html += section("Languages", d.languages, function (l) {
      return field(l.name, l.percentage != null ? l.percentage + "%" : "—");
    });
    html += section("Libraries", d.libraries, function (l) {
      return field(l.name + " (" + l.ecosystem + ")", l.version + (l.scope ? " · " + l.scope : ""));
    });
    html += section("Infrastructure", d.infrastructure, function (i) {
      return field(i.name + " (" + i.kind + ")", (i.version || "") + (i.usage ? " · " + i.usage : ""));
    });
    html += section("Dependencies", d.dependencies, function (dep) {
      return field(dep.target_name, (dep.kind || "") + (dep.description ? " · " + dep.description : ""));
    });
    html += section("Access", d.access, function (a) {
      return field(a.principal_type + ": " + a.principal_name, a.access_level);
    });
    html += section("Applications", d.applications, function (a) {
      var extra = a.usage || a.access_level || a.version || "";
      return '<div class="flex gap-2 text-sm"><span class="w-48">' + link(a) + "</span><span>" + esc(extra) + "</span></div>";
    });
    html += section("Versions", (d.versions || []).map(function (v) { return { v: v }; }), function (x) {
      return field("version", x.v);
    });
    html += section("Members", (d.members || []).map(function (m) { return { m: m }; }), function (x) {
      return field("user", x.m);
    });
    html += section("Groups", (d.groups || []).map(function (g) { return { g: g }; }), function (x) {
      return field("group", x.g);
    });

    $("#detail-body").html(html);
  }

  $(function () {
    $.getJSON("/api/platform/" + meta.entity + "/" + meta.id)
      .done(function (d) { render(d.detail); })
      .fail(function () { $("#detail-title").text("Not found"); });
  });
})(jQuery);
