function hiraethDisableSubmit(form, label) {
  const button = form.querySelector('button[type="submit"]');
  if (button) {
    button.disabled = true;
    if (label) {
      button.textContent = label;
    }
  }
  return true;
}

function hiraethConfirmSubmit(form, message, label) {
  if (!confirm(message)) {
    return false;
  }
  return hiraethDisableSubmit(form, label);
}

function hiraethDismissBanner(button, paramsToClear) {
  const banner = button.closest(".alert");
  if (banner) {
    banner.remove();
  }

  if (!window.history.replaceState || !paramsToClear) {
    return;
  }

  const url = new URL(window.location.href);
  paramsToClear.forEach((param) => url.searchParams.delete(param));
  const nextUrl = `${url.pathname}${url.search}${url.hash}`;
  window.history.replaceState({}, "", nextUrl);
}

async function hiraethCopyText(button) {
  const value = button.dataset.copyValue;
  if (!value) {
    return;
  }

  try {
    if (navigator.clipboard && window.isSecureContext) {
      await navigator.clipboard.writeText(value);
    } else {
      const textarea = document.createElement("textarea");
      textarea.value = value;
      textarea.setAttribute("readonly", "");
      textarea.style.position = "fixed";
      textarea.style.left = "-9999px";
      document.body.appendChild(textarea);
      textarea.select();
      document.execCommand("copy");
      textarea.remove();
    }

    const original = button.dataset.originalLabel || button.textContent;
    button.dataset.originalLabel = original;
    button.textContent = "Copied";
    window.setTimeout(() => {
      button.textContent = button.dataset.originalLabel;
    }, 1200);
  } catch (error) {
    console.error("Failed to copy text", error);
    button.textContent = "Copy failed";
  }
}

function hiraethToggleSecret(button) {
  const targetId = button.dataset.secretTarget;
  if (!targetId) {
    return;
  }

  const value = document.getElementById(targetId);
  if (!value) {
    return;
  }

  const isVisible = value.dataset.secretVisible === "true";
  value.dataset.secretVisible = isVisible ? "false" : "true";
  value.textContent = isVisible ? value.dataset.secretMasked : value.dataset.secretValue;
  button.textContent = isVisible ? "Reveal" : "Hide";
  button.setAttribute("aria-pressed", isVisible ? "false" : "true");
}

function hiraethFuzzyMatches(value, query) {
  if (!query) {
    return true;
  }

  let valueIndex = 0;
  let queryIndex = 0;
  const normalizedValue = value.toLowerCase();
  const normalizedQuery = query.toLowerCase().trim();

  while (
    valueIndex < normalizedValue.length &&
    queryIndex < normalizedQuery.length
  ) {
    if (normalizedValue[valueIndex] === normalizedQuery[queryIndex]) {
      queryIndex += 1;
    }
    valueIndex += 1;
  }

  return queryIndex === normalizedQuery.length;
}

function hiraethFilterTraceRequests(input) {
  const query = input.value || "";
  const rows = Array.from(document.querySelectorAll("[data-trace-row]"));
  let visibleCount = 0;

  rows.forEach((row) => {
    const requestId = row.dataset.traceRequestId || "";
    const isVisible = hiraethFuzzyMatches(requestId, query);
    row.classList.toggle("hidden", !isVisible);
    if (isVisible) {
      visibleCount += 1;
    }
  });

  const count = document.getElementById("trace-request-count");
  if (count) {
    const totalCount = count.dataset.totalCount || rows.length.toString();
    count.textContent = query.trim()
      ? `${visibleCount} / ${totalCount} shown`
      : `${totalCount} shown`;
  }

  const noMatches = document.getElementById("trace-request-no-matches");
  if (noMatches) {
    noMatches.classList.toggle("hidden", visibleCount !== 0 || rows.length === 0);
  }
}

function hiraethSelectTraceSpan(detailId, nodeId) {
  document.querySelectorAll("[data-trace-span-detail]").forEach((detail) => {
    detail.classList.toggle("hidden", detail.id !== detailId);
    detail.classList.toggle("block", detail.id === detailId);
  });

  document.querySelectorAll(".trace-graph-node").forEach((node) => {
    const isSelected = node.id === nodeId;
    node.classList.toggle("ring-2", isSelected);
    node.classList.toggle("ring-primary", isSelected);
    node.classList.toggle("ring-offset-1", isSelected);
    node.classList.toggle("ring-offset-base-100", isSelected);
    node.classList.toggle("shadow-md", isSelected);
    node.setAttribute("aria-pressed", isSelected ? "true" : "false");
  });

  window.hiraethSelectedTraceNodeId = nodeId;
  hiraethHighlightTraceConnections(nodeId, true);

  const empty = document.getElementById("trace-span-empty");
  if (empty) {
    empty.classList.add("hidden");
  }
}

function hiraethTraceConnectionIds(nodeId) {
  const ids = new Set([nodeId]);
  const node = document.getElementById(nodeId);
  const parentId = node ? node.dataset.parentNode : null;
  if (parentId) {
    ids.add(parentId);
  }

  const escapedNodeId = window.CSS && CSS.escape ? CSS.escape(nodeId) : nodeId;
  document.querySelectorAll(`[data-parent-node="${escapedNodeId}"]`).forEach((child) => {
    if (child.id) {
      ids.add(child.id);
    }
    if (child.dataset.childNode) {
      ids.add(child.dataset.childNode);
    }
  });

  return ids;
}

function hiraethClearTraceConnections() {
  document.querySelectorAll(".trace-graph-node").forEach((node) => {
    node.classList.remove("ring-1", "ring-accent", "brightness-110");
  });

  document.querySelectorAll("[data-trace-edge]").forEach((edge) => {
    edge.classList.remove("stroke-primary", "fill-primary");
    edge.classList.add(
      edge.tagName.toLowerCase() === "path"
        ? "stroke-base-content/35"
        : "fill-base-content/45",
    );
  });
}

function hiraethHighlightTraceConnections(nodeId, enabled) {
  if (!nodeId) {
    return;
  }

  if (!enabled) {
    hiraethClearTraceConnections();
    if (window.hiraethSelectedTraceNodeId && window.hiraethSelectedTraceNodeId !== nodeId) {
      hiraethHighlightTraceConnections(window.hiraethSelectedTraceNodeId, true);
    }
    return;
  }

  hiraethClearTraceConnections();
  const connectedIds = hiraethTraceConnectionIds(nodeId);

  connectedIds.forEach((connectedId) => {
    const connectedNode = document.getElementById(connectedId);
    if (connectedNode && connectedId !== nodeId) {
      connectedNode.classList.add("ring-1", "ring-accent", "brightness-110");
    }
  });

  document.querySelectorAll("[data-trace-edge]").forEach((edge) => {
    const parentId = edge.dataset.parentNode;
    const childId = edge.dataset.childNode;
    const isConnected =
      (parentId === nodeId && connectedIds.has(childId)) ||
      (childId === nodeId && connectedIds.has(parentId));

    if (isConnected) {
      edge.classList.remove("stroke-base-content/35", "fill-base-content/45");
      edge.classList.add(
        edge.tagName.toLowerCase() === "path" ? "stroke-primary" : "fill-primary",
      );
    }
  });
}

function hiraethOpenHashTarget() {
  if (!window.location.hash) {
    return;
  }

  const target = document.getElementById(window.location.hash.slice(1));
  if (target instanceof HTMLDetailsElement) {
    target.open = true;
    target.scrollIntoView({ block: "start" });
  }
}

function hiraethAutoDismissToasts() {
  document.querySelectorAll("[data-auto-dismiss]").forEach((toast) => {
    if (toast.dataset.dismissScheduled === "true") {
      return;
    }
    toast.dataset.dismissScheduled = "true";

    setTimeout(() => {
      toast.style.transition = "opacity 0.35s ease, transform 0.35s ease";
      toast.style.opacity = "0";
      toast.style.transform = "translateY(-12px)";
      setTimeout(() => {
        toast.remove();
      }, 350);
    }, 4000);
  });
}

function hiraethInitApp() {
  hiraethOpenHashTarget();
  hiraethAutoDismissToasts();
}

if (document.readyState === "loading") {
  document.addEventListener("DOMContentLoaded", () => hiraethInitApp());
} else {
  hiraethInitApp();
}

document.addEventListener("htmx:afterSwap", () => {
  hiraethInitApp();
});

window.addEventListener("hashchange", hiraethOpenHashTarget);
