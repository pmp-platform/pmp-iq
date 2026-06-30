// Settings page: render the accounts table and drive the CRUD/test/preview API.
(function ($) {
  "use strict";

  var accountsCache = [];
  var editId = null;

  // Parse a stored List selection_value (JSON array of names) defensively.
  function parseList(value) {
    try {
      var arr = JSON.parse(value || "[]");
      return Array.isArray(arr) ? arr : [];
    } catch (e) {
      return [];
    }
  }

  // Human-readable summary of an account's selection for the table.
  function selectionLabel(a) {
    if (a.selection_mode === "regex") return "regex: " + (a.selection_value || "");
    if (a.selection_mode === "list") return "list: " + parseList(a.selection_value).join(", ");
    return "all";
  }

  function render(accounts) {
    accountsCache = accounts;
    var $body = $("#accounts-table tbody").empty();
    if (!accounts.length) {
      $body.append('<tr><td class="p-3 text-slate-400" colspan="5">No accounts yet.</td></tr>');
      return;
    }
    accounts.forEach(function (a) {
      var $row = $(
        '<tr class="border-b">' +
          '<td class="p-3">' + $("<div>").text(a.name).html() + "</td>" +
          "<td>" + a.provider_type + "</td>" +
          "<td>" + $("<div>").text(selectionLabel(a)).html() + "</td>" +
          "<td>" + window.PI.badge(a.enabled ? "Enabled" : "Disabled", a.enabled ? "success" : "danger") + "</td>" +
          '<td class="text-right pr-3 whitespace-nowrap">' +
            window.PI.actionButton("Edit", { "data-act": "edit" }) +
            window.PI.actionButton("Test", { "data-act": "test" }) +
            window.PI.actionButton("Repos", { "data-act": "repos" }) +
            window.PI.actionButton("Delete", { "data-act": "del" }, "danger") +
          "</td>" +
        "</tr>"
      );
      $row.find("button").data("id", a.id);
      $body.append($row);
    });
  }

  function load() {
    $.getJSON("/api/settings/accounts").done(function (d) { render(d.accounts); });
  }

  function flash(msg, ok) {
    $("#account-msg").text(msg).css("color", ok ? "#15803d" : "#b91c1c");
  }

  // One removable text input for a repository name.
  function repoRow(value) {
    var $row = $(
      '<div class="flex gap-2">' +
        '<input class="repo-name flex-1 border rounded px-3 py-2" placeholder="owner/name or name" />' +
        '<button type="button" class="remove-repo border rounded px-2 text-slate-500">✕</button>' +
      "</div>"
    );
    $row.find(".repo-name").val(value || "");
    return $row;
  }

  // Show only the inputs relevant to the chosen selection mode.
  function updateSelectionUI(mode) {
    $("#regex-field").toggleClass("hidden", mode !== "regex");
    $("#list-field").toggleClass("hidden", mode !== "list");
    if (mode === "list" && $("#repo-list").children().length === 0) {
      $("#repo-list").append(repoRow(""));
    }
  }

  // Collect selection_value in the format the backend expects for the mode.
  function selectionValue(mode) {
    if (mode === "regex") return $("#regex-value").val().trim() || null;
    if (mode === "list") {
      var names = [];
      $("#repo-list .repo-name").each(function () {
        var v = $(this).val().trim();
        if (v) names.push(v);
      });
      return names.length ? JSON.stringify(names) : null;
    }
    return null;
  }

  // Reset the modal to "add" mode.
  function openAdd() {
    editId = null;
    $("#account-modal-title").text("Add repository account");
    $("#account-form")[0].reset();
    $("#account-form [name=token]").attr("placeholder", "Token (stored encrypted)");
    $("#regex-value").val("");
    $("#repo-list").empty();
    flash("", true);
    updateSelectionUI($("#selection-mode").val());
  }

  // Prefill the modal from an existing account for "edit" mode.
  function openEdit(a) {
    editId = a.id;
    $("#account-modal-title").text("Edit repository account");
    var $f = $("#account-form");
    $f[0].reset();
    $f.find("[name=name]").val(a.name);
    $f.find("[name=provider_type]").val(a.provider_type);
    $f.find("[name=auth_type]").val(a.auth_type);
    $f.find("[name=base_url]").val(a.base_url || "");
    $f.find("[name=token]").val("").attr("placeholder", "Token (leave blank to keep current)");
    $f.find("[name=selection_mode]").val(a.selection_mode);
    $("#regex-value").val(a.selection_mode === "regex" ? (a.selection_value || "") : "");
    $("#repo-list").empty();
    if (a.selection_mode === "list") {
      var names = parseList(a.selection_value);
      if (!names.length) names = [""];
      names.forEach(function (n) { $("#repo-list").append(repoRow(n)); });
    }
    flash("", true);
    updateSelectionUI(a.selection_mode);
    window.PI.openModal("#account-modal");
  }

  $(function () {
    var initial = $("#accounts-data").text();
    try { render(JSON.parse(initial)); } catch (e) { load(); }

    $("#add-account-btn").on("click", openAdd);
    window.PI.refreshButton(load).insertBefore("#add-account-btn");
    $("#selection-mode").on("change", function () { updateSelectionUI($(this).val()); });
    $("#add-repo").on("click", function () { $("#repo-list").append(repoRow("")); });
    $("#repo-list").on("click", ".remove-repo", function () { $(this).closest("div").remove(); });

    $("#account-form").on("submit", function (e) {
      e.preventDefault();
      var data = {};
      $.each($(this).serializeArray(), function (_, f) { data[f.name] = f.value; });
      data.enabled = true;
      data.selection_value = selectionValue(data.selection_mode);
      if (!data.token) delete data.token; // blank token keeps the existing secret
      var url = editId ? "/api/settings/accounts/" + editId : "/api/settings/accounts";
      $.ajax({
        url: url,
        method: editId ? "PUT" : "POST",
        contentType: "application/json",
        data: JSON.stringify(data),
      }).done(function () {
        flash("", true);
        load();
        window.PI.closeModal("#account-modal");
      }).fail(function (x) { flash("Error: " + x.responseText, false); });
    });

    $("#accounts-table").on("click", "button", function () {
      var id = $(this).data("id");
      var act = $(this).data("act");
      if (act === "edit") {
        var a = accountsCache.filter(function (x) { return x.id === id; })[0];
        if (a) openEdit(a);
      } else if (act === "del") {
        window.PI.confirm("Delete this account?", function () {
          $.ajax({ url: "/api/settings/accounts/" + id, method: "DELETE" }).done(load);
        });
      } else if (act === "test") {
        $.ajax({ url: "/api/settings/accounts/" + id + "/validate", method: "POST" })
          .done(function () { window.PI.toast("Connection OK", true); })
          .fail(function (x) { window.PI.toast("Failed: " + x.responseText, false); });
      } else if (act === "repos") {
        $.getJSON("/api/settings/accounts/" + id + "/repositories")
          .done(function (d) { window.PI.toast(d.repositories.length + " repositories selected", true); })
          .fail(function (x) { window.PI.toast("Failed: " + x.responseText, false); });
      }
    });
  });
})(jQuery);
