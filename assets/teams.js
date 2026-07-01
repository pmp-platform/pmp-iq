// Settings → Teams & roles (M37, admin only). Manage teams + role assignments.
(function ($) {
  "use strict";

  function api(method, url, body) {
    return $.ajax({ url: url, method: method, contentType: "application/json", data: body ? JSON.stringify(body) : undefined });
  }

  function loadTeams() {
    api("GET", "/api/teams").done(function (d) {
      var $root = $("#teams-root").empty();
      (d.teams || []).forEach(function (t) {
        var $del = $('<button class="text-xs text-blue-600 hover:underline ml-2">').text("delete")
          .on("click", function () { api("DELETE", "/api/teams/" + t.id).always(loadTeams); });
        $root.append($('<div class="py-0.5 border-b border-slate-100 flex justify-between">')
          .append($('<span>').text(t.name + (t.tenant_id ? " · tenant " + t.tenant_id : "")))
          .append($del));
      });
      $root.append(createTeamForm());
    }).fail(function () { $("#teams-root").html('<div class="text-xs text-slate-400">Admin only.</div>'); });
  }

  function createTeamForm() {
    var $name = $('<input class="border rounded text-xs p-1" placeholder="team name">');
    var $tenant = $('<input class="border rounded text-xs p-1 w-24" placeholder="tenant (opt)">');
    var $add = $('<button class="text-xs bg-blue-600 text-white rounded px-2 py-1">').text("Add team")
      .on("click", function () {
        var payload = { name: $name.val() };
        if ($tenant.val()) { payload.tenant_id = $tenant.val(); }
        api("POST", "/api/teams", payload).done(loadTeams).fail(function (x) { alert("Failed: " + (x.responseText || x.status)); });
      });
    return $('<div class="flex gap-1 mt-2">').append($name).append($tenant).append($add);
  }

  function loadRoles() {
    api("GET", "/api/roles").done(function (d) {
      var $root = $("#roles-root").empty();
      (d.roles || []).forEach(function (r) {
        $root.append($('<div class="py-0.5 border-b border-slate-100">').text(r.principal + " → " + r.role));
      });
      $root.append(setRoleForm());
    }).fail(function () { $("#roles-root").html('<div class="text-xs text-slate-400">Admin only.</div>'); });
  }

  function setRoleForm() {
    var $who = $('<input class="border rounded text-xs p-1" placeholder="principal">');
    var $role = $('<select class="border rounded text-xs p-1">');
    ["viewer", "maintainer", "admin"].forEach(function (r) { $role.append($('<option>').val(r).text(r)); });
    var $set = $('<button class="text-xs bg-blue-600 text-white rounded px-2 py-1">').text("Set")
      .on("click", function () {
        api("POST", "/api/roles", { principal: $who.val(), role: $role.val() }).done(loadRoles)
          .fail(function (x) { alert("Failed: " + (x.responseText || x.status)); });
      });
    return $('<div class="flex gap-1 mt-2">').append($who).append($role).append($set);
  }

  $(function () { loadTeams(); loadRoles(); });
})(jQuery);
