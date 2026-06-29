// File viewing for an application's cloned checkout. Provides:
//  - PIFiles.mount($panel): the File Explorer tab (lazy tree + viewer).
//  - PIFiles.openFiles({title, files}): a modal listing a specific set of files
//    (e.g. a component's files) with the same viewer.
// The viewer highlights code with CodeMirror and renders Markdown with marked.
(function ($) {
  "use strict";

  var meta = JSON.parse($("#detail-meta").text());
  if (meta.entity !== "applications") {
    window.PIFiles = { mount: function () {}, openFiles: function () {} };
    return;
  }

  var base = "/api/platform/applications/" + meta.id + "/files";

  function esc(v) {
    return $("<div>").text(v === null || v === undefined ? "" : v).html();
  }
  function isMarkdown(name) {
    return /\.(md|markdown|mdown)$/i.test(name);
  }

  // A reusable read-only viewer built into `$wrap`; returns { open(path) }.
  function createViewer($wrap) {
    $wrap.html(
      '<div class="flex flex-col min-w-0 h-full">' +
        '<div class="v-path text-xs text-slate-500 px-3 py-1.5 border-b border-slate-100 font-mono truncate">Select a file…</div>' +
        '<div class="flex-1 min-h-0 overflow-auto">' +
          '<div class="v-code h-full"><textarea></textarea></div>' +
          '<div class="v-rendered md-body hidden max-w-none p-4 text-sm"></div>' +
        "</div>" +
      "</div>"
    );
    var $path = $wrap.find(".v-path");
    var $code = $wrap.find(".v-code");
    var $rendered = $wrap.find(".v-rendered");
    var editor = null;

    function ensureEditor() {
      if (editor || typeof CodeMirror === "undefined") return editor;
      editor = CodeMirror.fromTextArea($code.find("textarea")[0], {
        lineNumbers: true, readOnly: true, viewportMargin: Infinity,
      });
      return editor;
    }
    function modeFor(path) {
      var name = path.split("/").pop();
      if (CodeMirror.findModeByFileName) {
        var info = CodeMirror.findModeByFileName(name);
        if (info) return info.mime || info.mode;
      }
      return null;
    }
    function showCode(path, content) {
      $rendered.addClass("hidden");
      $code.removeClass("hidden");
      var cm = ensureEditor();
      if (!cm) { $code.text(content); return; }
      cm.setOption("mode", modeFor(path));
      cm.setValue(content || "");
      cm.scrollTo(0, 0);
      setTimeout(function () { cm.refresh(); }, 0);
    }
    function showMarkdown(content) {
      $code.addClass("hidden");
      $rendered.removeClass("hidden");
      if (typeof marked === "undefined") { $rendered.text(content); return; }
      var $html = $("<div>").html(marked.parse(content || ""));
      $html.find("script, style, iframe").remove();
      $rendered.empty().append($html);
    }
    function open(path) {
      $path.text(path);
      $.getJSON(base + "/content", { path: path })
        .done(function (d) {
          if (isMarkdown(path)) showMarkdown(d.content);
          else showCode(path, d.content);
        })
        .fail(function (xhr) {
          var err = xhr.responseJSON && xhr.responseJSON.error;
          showCode(path, "// " + ((err && err.message) || "could not open file"));
        });
    }
    return { open: open };
  }

  // ---- File Explorer tab: a lazy tree on the left, a viewer on the right -----

  function icon(isDir, open) {
    if (isDir) return open ? "📂 " : "📁 ";
    return "📄 ";
  }

  function renderLevel($into, viewer, path, depth) {
    $.getJSON(base, { path: path }).done(function (d) {
      (d.entries || []).forEach(function (entry) {
        var rel = path ? path + "/" + entry.name : entry.name;
        var $row = $('<div class="cursor-pointer hover:bg-slate-100 rounded px-1 py-0.5 truncate select-none"></div>')
          .css("padding-left", depth * 14 + 4 + "px")
          .text(icon(entry.is_dir, false) + entry.name);
        $into.append($row);
        if (entry.is_dir) {
          var $children = $('<div class="hidden"></div>');
          var loaded = false;
          $into.append($children);
          $row.on("click", function () {
            var nowHidden = $children.toggleClass("hidden").hasClass("hidden");
            $row.text(icon(true, !nowHidden) + entry.name);
            if (!loaded) { loaded = true; renderLevel($children, viewer, rel, depth + 1); }
          });
        } else {
          $row.on("click", function () { viewer.open(rel); });
        }
      });
    });
  }

  function mount($panel) {
    if ($panel.data("files-mounted")) return;
    $panel.data("files-mounted", true);
    $panel.html(
      '<div class="bg-white rounded-lg shadow border border-slate-200 overflow-hidden">' +
        '<div class="flex" style="height: 32rem;">' +
          '<div class="v-tree w-72 shrink-0 overflow-auto border-r border-slate-200 p-2 text-sm font-mono"></div>' +
          '<div class="v-viewer flex-1 min-w-0"></div>' +
        "</div>" +
      "</div>"
    );
    var viewer = createViewer($panel.find(".v-viewer"));
    renderLevel($panel.find(".v-tree"), viewer, "", 0);
  }

  // ---- A modal listing a specific set of files (e.g. a component's files) ----

  var $modal = null;
  function ensureModal() {
    if ($modal) return $modal;
    $modal = $(
      '<div id="files-modal" class="modal hidden fixed inset-0 bg-black/40 z-50 items-center justify-center p-4">' +
        '<div class="bg-white rounded-lg shadow-lg w-[90vw] max-w-5xl h-[85vh] flex flex-col">' +
          '<div class="flex items-center justify-between p-4 border-b border-slate-200">' +
            '<h3 class="files-modal-title text-lg font-semibold">Files</h3>' +
            '<button type="button" data-modal-close class="text-2xl leading-none text-slate-400 hover:text-slate-700">&times;</button>' +
          "</div>" +
          '<div class="flex flex-1 min-h-0">' +
            '<div class="files-modal-list w-72 shrink-0 overflow-auto border-r border-slate-200 p-2 text-sm font-mono"></div>' +
            '<div class="files-modal-viewer flex-1 min-w-0"></div>' +
          "</div>" +
        "</div>" +
      "</div>"
    ).appendTo("body");
    return $modal;
  }

  function openFiles(opts) {
    ensureModal();
    $modal.find(".files-modal-title").text(opts.title ? opts.title + " — files" : "Files");
    var $list = $modal.find(".files-modal-list").empty();
    var viewer = createViewer($modal.find(".files-modal-viewer"));
    var files = opts.files || [];
    if (!files.length) {
      $list.html('<div class="text-slate-400 p-1">No files recorded for this component.</div>');
    }
    files.forEach(function (p) {
      var $row = $('<div class="cursor-pointer hover:bg-slate-100 rounded px-1 py-0.5 truncate"></div>')
        .attr("title", p)
        .text("📄 " + p.split("/").pop());
      $row.on("click", function () {
        $list.find(".sel").removeClass("sel bg-blue-50 text-blue-700");
        $row.addClass("sel bg-blue-50 text-blue-700");
        viewer.open(p);
      });
      $list.append($row);
    });
    PI.openModal("#files-modal");
    if (files.length) {
      $list.children().first().addClass("sel bg-blue-50 text-blue-700");
      viewer.open(files[0]);
    }
  }

  window.PIFiles = { mount: mount, openFiles: openFiles };
})(jQuery);
