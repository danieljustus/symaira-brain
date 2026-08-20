(function() {
  "use strict";

  const API_BASE = window.location.origin;

  const state = {
    token: sessionStorage.getItem("symmemory_token") || "",
    authenticated: false
  };

  const $ = (sel) => document.querySelector(sel);
  const $$ = (sel) => document.querySelectorAll(sel);

  function getToken() {
    return state.token;
  }

  function setToken(token) {
    state.token = token;
    sessionStorage.setItem("symmemory_token", token);
  }

  function clearToken() {
    state.token = "";
    state.authenticated = false;
    sessionStorage.removeItem("symmemory_token");
  }

  async function apiFetch(path, options) {
    options = options || {};
    options.headers = options.headers || {};
    if (state.token) {
      options.headers["Authorization"] = "Bearer " + state.token;
    }
    const res = await fetch(API_BASE + path, options);
    return res;
  }

  function setAuthStatus(msg, isError) {
    const el = $("#auth-status");
    el.textContent = msg;
    el.className = isError ? "error" : "";
  }

  function setStatus(msg, cls) {
    const el = $("#status-text");
    el.textContent = msg;
    el.className = cls || "";
  }

  async function checkAuth() {
    if (!state.token) {
      setAuthStatus("No token", true);
      setStatus("Disconnected", "error");
      return false;
    }
    try {
      const res = await apiFetch("/api/status");
      if (res.ok) {
        state.authenticated = true;
        setAuthStatus("Authenticated", false);
        setStatus("Connected", "connected");
        return true;
      } else if (res.status === 401) {
        state.authenticated = false;
        setAuthStatus("Unauthorized", true);
        setStatus("Unauthorized", "error");
        return false;
      }
    } catch (e) {
      setAuthStatus("Connection failed", true);
      setStatus("Disconnected", "error");
    }
    return false;
  }

  async function loadMemories() {
    const scope = $("#scope-filter").value;
    const url = "/api/list" + (scope ? "?scope=" + encodeURIComponent(scope) : "");
    const res = await apiFetch(url);
    if (res.status === 401) {
      setAuthStatus("Unauthorized", true);
      state.authenticated = false;
      return;
    }
    if (!res.ok) return;
    const memories = await res.json();
    const tbody = $("#memories-body");
    tbody.innerHTML = "";
    const emptyMsg = $("#memories-empty");

    if (!memories || memories.length === 0) {
      emptyMsg.classList.add("visible");
      return;
    }
    emptyMsg.classList.remove("visible");

    memories.forEach(function(m) {
      const tr = document.createElement("tr");
      const updated = m.updated_at ? new Date(m.updated_at).toLocaleString() : "";
      tr.innerHTML =
        '<td class="id-cell">' + escapeHtml(m.id.substring(0, 8)) + "...</td>" +
        '<td class="content-cell" title="' + escapeHtml(m.content) + '">' + escapeHtml(m.content) + "</td>" +
        "<td>" + escapeHtml(m.scope) + "</td>" +
        "<td>" + escapeHtml(updated) + "</td>" +
        '<td><button class="delete-btn" data-id="' + escapeHtml(m.id) + '">Delete</button></td>';
      tbody.appendChild(tr);
    });
  }

  async function doSearch() {
    const query = $("#search-query").value.trim();
    if (!query) return;
    const scope = $("#search-scope").value;
    const body = { query: query };
    if (scope) body.scope = scope;

    const res = await apiFetch("/api/search", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify(body)
    });
    if (res.status === 401) {
      setAuthStatus("Unauthorized", true);
      state.authenticated = false;
      return;
    }
    if (!res.ok) return;
    const results = await res.json();
    const tbody = $("#search-body");
    tbody.innerHTML = "";
    const emptyMsg = $("#search-empty");

    if (!results || results.length === 0) {
      emptyMsg.classList.add("visible");
      return;
    }
    emptyMsg.classList.remove("visible");

    results.forEach(function(m) {
      const tr = document.createElement("tr");
      const score = m.score != null ? m.score.toFixed(4) : "-";
      tr.innerHTML =
        '<td class="id-cell">' + escapeHtml(m.id.substring(0, 8)) + "...</td>" +
        '<td class="content-cell" title="' + escapeHtml(m.content) + '">' + escapeHtml(m.content) + "</td>" +
        "<td>" + escapeHtml(m.scope) + "</td>" +
        '<td class="score-cell">' + escapeHtml(score) + "</td>";
      tbody.appendChild(tr);
    });
  }

  async function loadRules() {
    const res = await apiFetch("/api/rules");
    if (res.status === 401) {
      setAuthStatus("Unauthorized", true);
      state.authenticated = false;
      return;
    }
    if (!res.ok) return;
    const data = await res.json();
    const rules = data.rules || [];
    const tbody = $("#rules-body");
    tbody.innerHTML = "";
    const emptyMsg = $("#rules-empty");

    if (rules.length === 0) {
      emptyMsg.classList.add("visible");
      return;
    }
    emptyMsg.classList.remove("visible");

    rules.forEach(function(r) {
      const tr = document.createElement("tr");
      const created = r.created_at ? new Date(r.created_at).toLocaleString() : "";
      tr.innerHTML =
        '<td class="id-cell">' + escapeHtml(r.id.substring(0, 8)) + "...</td>" +
        '<td class="content-cell" title="' + escapeHtml(r.content) + '">' + escapeHtml(r.content) + "</td>" +
        "<td>" + escapeHtml(r.scope) + "</td>" +
        "<td>" + escapeHtml(created) + "</td>";
      tbody.appendChild(tr);
    });
  }

  async function loadEntities() {
    const res = await apiFetch("/api/entities");
    if (res.status === 401) {
      setAuthStatus("Unauthorized", true);
      state.authenticated = false;
      return;
    }
    if (!res.ok) return;
    const data = await res.json();
    const entities = data.entities || [];
    const tbody = $("#entities-body");
    tbody.innerHTML = "";
    const emptyMsg = $("#entities-empty");

    if (entities.length === 0) {
      emptyMsg.classList.add("visible");
      return;
    }
    emptyMsg.classList.remove("visible");

    entities.forEach(function(e) {
      const tr = document.createElement("tr");
      const created = e.created_at ? new Date(e.created_at).toLocaleString() : "";
      tr.innerHTML =
        '<td class="content-cell">' + escapeHtml(e.name) + "</td>" +
        "<td>" + escapeHtml(e.type) + "</td>" +
        '<td class="content-cell" title="' + escapeHtml(e.description || "") + '">' + escapeHtml(e.description || "-") + "</td>" +
        "<td>" + escapeHtml(created) + "</td>";
      tbody.appendChild(tr);
    });
  }

  async function addMemory() {
    const content = $("#memory-content").value.trim();
    if (!content) {
      $("#add-memory-status").textContent = "Content is required";
      $("#add-memory-status").className = "error";
      return;
    }

    const scope = $("#memory-scope").value;
    const body = { content: content, scope: scope };

    const metaKey = $("#memory-meta-key").value.trim();
    const metaValue = $("#memory-meta-value").value.trim();
    if (metaKey) {
      body.metadata = {};
      body.metadata[metaKey] = metaValue;
    }

    const res = await apiFetch("/api/set", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify(body)
    });

    if (res.status === 401) {
      setAuthStatus("Unauthorized", true);
      state.authenticated = false;
      return;
    }

    if (res.ok) {
      $("#add-memory-status").textContent = "Memory added successfully";
      $("#add-memory-status").className = "success";
      $("#memory-content").value = "";
      $("#memory-meta-key").value = "";
      $("#memory-meta-value").value = "";
      loadMemories();
    } else {
      const data = await res.json();
      $("#add-memory-status").textContent = data.error || "Failed to add memory";
      $("#add-memory-status").className = "error";
    }
  }

  async function deleteMemory(id) {
    if (!confirm("Delete memory " + id + "?")) return;
    const res = await apiFetch("/api/delete?id=" + encodeURIComponent(id), {
      method: "DELETE"
    });
    if (res.ok) {
      loadMemories();
    }
  }

  function escapeHtml(str) {
    if (!str) return "";
    return str
      .replace(/&/g, "&amp;")
      .replace(/</g, "&lt;")
      .replace(/>/g, "&gt;")
      .replace(/"/g, "&quot;");
  }

  function switchTab(tabName) {
    $$(".tab-btn").forEach(function(btn) {
      btn.classList.toggle("active", btn.getAttribute("data-tab") === tabName);
    });
    $$(".tab-content").forEach(function(section) {
      section.classList.toggle("active", section.id === tabName + "-tab");
    });

    if (tabName === "memories") loadMemories();
    if (tabName === "rules") loadRules();
    if (tabName === "entities") loadEntities();
  }

  function init() {
    if (state.token) {
      $("#token-input").value = state.token;
      checkAuth().then(function(ok) {
        if (ok) loadMemories();
      });
    }

    $("#auth-btn").addEventListener("click", function() {
      const token = $("#token-input").value.trim();
      if (!token) {
        clearToken();
        setAuthStatus("No token", true);
        setStatus("Disconnected", "error");
        return;
      }
      setToken(token);
      checkAuth().then(function(ok) {
        if (ok) loadMemories();
      });
    });

    $$(".tab-btn").forEach(function(btn) {
      btn.addEventListener("click", function() {
        switchTab(this.getAttribute("data-tab"));
      });
    });

    $("#scope-filter").addEventListener("change", function() {
      loadMemories();
    });

    $("#refresh-btn").addEventListener("click", function() {
      loadMemories();
    });

    $("#search-btn").addEventListener("click", function() {
      doSearch();
    });

    $("#search-query").addEventListener("keydown", function(e) {
      if (e.key === "Enter") doSearch();
    });

    $("#memories-body").addEventListener("click", function(e) {
      if (e.target.classList.contains("delete-btn")) {
        deleteMemory(e.target.getAttribute("data-id"));
      }
    });

    $("#add-memory-btn").addEventListener("click", function() {
      addMemory();
    });
  }

  if (document.readyState === "loading") {
    document.addEventListener("DOMContentLoaded", init);
  } else {
    init();
  }
})();
