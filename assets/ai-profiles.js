// Settings page: AI agent profiles table + CRUD/test API.
(function ($) {
  "use strict";

  var MODELS = [
    "claude-opus-4-8",
    "claude-fable-5",
    "claude-opus-4-7",
    "claude-opus-4-6",
    "claude-sonnet-4-6",
    "claude-haiku-4-5",
  ];
  var EFFORTS = ["low", "medium", "high", "xhigh", "max"];

  // Lookup of profile id -> profile (for populating the Edit form). The id of the
  // profile currently being edited, or null when creating a new one.
  var profilesById = {};
  var editingId = null;

  function render(profiles) {
    profilesById = {};
    var $body = $("#profiles-table tbody").empty();
    if (!profiles.length) {
      $body.append('<tr><td class="p-3 text-slate-400" colspan="4">No profiles yet.</td></tr>');
      return;
    }
    profiles.forEach(function (p) {
      profilesById[p.id] = p;
      var $row = $(
        '<tr class="border-b">' +
          '<td class="p-3">' + $("<div>").text(p.name).html() + "</td>" +
          "<td>" + p.provider_type + "</td>" +
          "<td>" + window.PI.badge(p.enabled ? "Enabled" : "Disabled", p.enabled ? "success" : "danger") + "</td>" +
          '<td class="text-right pr-3 whitespace-nowrap">' +
            window.PI.actionButton("Edit", { "data-act": "edit" }) +
            window.PI.actionButton("Test", { "data-act": "test" }) +
            window.PI.actionButton("Validate", { "data-act": "validate" }) +
            window.PI.actionButton("Delete", { "data-act": "del" }, "danger") +
          "</td>" +
        "</tr>"
      );
      $row.find("button").data("id", p.id);
      $body.append($row);
    });
  }

  function load() {
    $.getJSON("/api/settings/ai-profiles").done(function (d) { render(d.profiles); });
  }

  function flash(msg, ok) {
    $("#profile-msg").text(msg).css("color", ok ? "#15803d" : "#b91c1c");
  }

  function fillSelect(sel, values, leadingLabel) {
    var $sel = $(sel).empty();
    if (leadingLabel) $sel.append($("<option>").val("").text(leadingLabel));
    values.forEach(function (v) { $sel.append($("<option>").val(v).text(v)); });
  }

  // Show only the fields relevant to the chosen provider and tailor the API-key help.
  function updateProviderUI() {
    var provider = $("#profile-provider").val();
    var isAnthropic = provider === "anthropic";
    // Effort applies to both providers, so it stays visible for either.
    $("#max-tokens-field, #base-url-field").toggleClass("hidden", !isAnthropic);
    $("#binary-path-field, #extra-args-field").toggleClass("hidden", isAnthropic);
    if (isAnthropic) {
      $("#profile-api-key").attr("placeholder", "API key (required, stored encrypted)");
      $("#api-key-help").text("Your Anthropic API key. Stored encrypted at rest.");
    } else {
      $("#profile-api-key").attr("placeholder", "API key (optional)");
      $("#api-key-help").text(
        "Optional. If left blank, the Claude CLI uses its own configured authentication " +
        "(e.g. `claude login`, or ANTHROPIC_API_KEY in its environment). Set a key only to " +
        "override that for this profile."
      );
    }
  }

  // Build the provider-specific config object from the form fields.
  function buildConfig(provider) {
    var config = {};
    var model = $("#profile-model").val();
    if (model) config.model = model;
    var effort = $("#profile-effort").val();
    if (effort) config.effort = effort;
    if (provider === "anthropic") {
      var maxTokens = $("#profile-max-tokens").val();
      if (maxTokens) config.max_tokens = parseInt(maxTokens, 10);
      var baseUrl = $("#profile-base-url").val().trim();
      if (baseUrl) config.base_url = baseUrl;
    } else {
      var binaryPath = $("#profile-binary-path").val().trim();
      if (binaryPath) config.binary_path = binaryPath;
      var extraArgs = $("#profile-extra-args").val().trim();
      if (extraArgs) config.extra_args = extraArgs.split(/\s+/);
    }
    return config;
  }

  // Reset the modal back to "create" mode (also called after a successful save).
  function resetToCreate() {
    editingId = null;
    $("#profile-modal-title").text("Add AI profile");
    $("#profile-form")[0].reset();
    $("#profile-enabled").prop("checked", true);
    flash("", true);
    updateProviderUI();
  }

  // Populate the modal from an existing profile and switch it to "edit" mode.
  function openEdit(p) {
    if (!p) return;
    editingId = p.id;
    $("#profile-modal-title").text("Edit AI profile");
    $("#profile-form")[0].reset();
    $("#profile-form").find("[name=name]").val(p.name);
    $("#profile-provider").val(p.provider_type);
    updateProviderUI();
    var c = p.config || {};
    $("#profile-model").val(c.model || "");
    $("#profile-effort").val(c.effort || "");
    $("#profile-max-tokens").val(c.max_tokens != null ? c.max_tokens : "");
    $("#profile-base-url").val(c.base_url || "");
    $("#profile-binary-path").val(c.binary_path || "");
    $("#profile-extra-args").val((c.extra_args || []).join(" "));
    $("#profile-enabled").prop("checked", !!p.enabled);
    $("#profile-api-key").val("");
    if (p.has_secret) {
      $("#profile-api-key").attr("placeholder", "Leave blank to keep current key");
    }
    flash("", true);
    window.PI.openModal("#profile-modal");
  }

  $(function () {
    load();
    fillSelect("#profile-model", MODELS, "Default model");
    fillSelect("#profile-effort", EFFORTS, "Default effort");
    updateProviderUI();
    $("#profile-provider").on("change", updateProviderUI);
    // The "Add AI profile" button (data-modal-open) opens the modal; clear any
    // prior edit state so it always opens in create mode.
    $("#add-profile-btn").on("click", resetToCreate);
    window.PI.refreshButton(load).insertBefore("#add-profile-btn");

    $("#profile-form").on("submit", function (e) {
      e.preventDefault();
      var provider = $("#profile-provider").val();
      var apiKey = $("#profile-api-key").val();
      // Anthropic API requires a key. On edit, a blank key is allowed only when
      // the stored profile already has one (the server keeps the existing secret).
      if (provider === "anthropic" && !apiKey) {
        var existing = editingId ? profilesById[editingId] : null;
        if (!existing || !existing.has_secret) {
          flash("An API key is required for Anthropic API profiles.", false);
          return;
        }
      }
      var data = {
        name: $(this).find("[name=name]").val(),
        provider_type: provider,
        config: buildConfig(provider),
        enabled: $("#profile-enabled").is(":checked"),
      };
      if (apiKey) data.api_key = apiKey;
      $.ajax({
        url: editingId ? "/api/settings/ai-profiles/" + editingId : "/api/settings/ai-profiles",
        method: editingId ? "PUT" : "POST",
        contentType: "application/json",
        data: JSON.stringify(data),
      }).done(function () {
        flash("", true);
        load();
        resetToCreate();
        window.PI.closeModal("#profile-modal");
      }).fail(function (x) { flash("Error: " + x.responseText, false); });
    });

    $("#profiles-table").on("click", "button", function () {
      var id = $(this).data("id");
      var act = $(this).data("act");
      if (act === "edit") {
        openEdit(profilesById[id]);
      } else if (act === "del") {
        window.PI.confirm("Delete this AI profile?", function () {
          $.ajax({ url: "/api/settings/ai-profiles/" + id, method: "DELETE" }).done(load);
        });
      } else if (act === "validate") {
        $.ajax({ url: "/api/settings/ai-profiles/" + id + "/validate", method: "POST" })
          .done(function () { window.PI.toast("Profile OK", true); })
          .fail(function (x) { window.PI.toast("Failed: " + x.responseText, false); });
      } else if (act === "test") {
        var prompt = window.prompt("Test prompt:", "Say hello in one word.");
        if (!prompt) return;
        $.ajax({
          url: "/api/settings/ai-profiles/" + id + "/test",
          method: "POST",
          contentType: "application/json",
          data: JSON.stringify({ prompt: prompt }),
        }).done(function (d) { window.PI.toast("Response: " + d.response.text, true); })
          .fail(function (x) { window.PI.toast("Failed: " + x.responseText, false); });
      }
    });
  });
})(jQuery);
