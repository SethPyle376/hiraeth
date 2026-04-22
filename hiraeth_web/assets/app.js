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

function hiraethCollapseStorageKey(detail) {
  const collapseKey = detail.dataset.collapseKey || detail.id;
  if (!collapseKey) {
    return null;
  }

  return `hiraeth:collapse:${window.location.pathname}:${collapseKey}`;
}

function hiraethReadStorage(key) {
  try {
    return window.localStorage.getItem(key);
  } catch (_error) {
    return null;
  }
}

function hiraethWriteStorage(key, value) {
  try {
    window.localStorage.setItem(key, value);
  } catch (_error) {
    // Ignore storage failures so the UI keeps working in private or restricted contexts.
  }
}

function hiraethBindCollapsible(detail) {
  if (detail.dataset.collapsePersistBound === "true") {
    return;
  }

  const storageKey = hiraethCollapseStorageKey(detail);
  if (!storageKey) {
    return;
  }

  const priority = detail.dataset.collapsePriority || "saved";
  const savedState = hiraethReadStorage(storageKey);

  if (priority !== "server" && savedState !== null) {
    detail.open = savedState === "open";
  }

  hiraethWriteStorage(storageKey, detail.open ? "open" : "closed");
  detail.addEventListener("toggle", () => {
    hiraethWriteStorage(storageKey, detail.open ? "open" : "closed");
  });
  detail.dataset.collapsePersistBound = "true";
}

function hiraethInitCollapsibles(root) {
  if (!(root instanceof Element) && root !== document) {
    return;
  }

  const details = [];
  if (root instanceof Element && root.matches("details[data-collapse-persist]")) {
    details.push(root);
  }

  details.push(...root.querySelectorAll("details[data-collapse-persist]"));
  details.forEach(hiraethBindCollapsible);
}

function hiraethInitApp(root) {
  hiraethInitCollapsibles(root);
}

if (document.readyState === "loading") {
  document.addEventListener("DOMContentLoaded", () => hiraethInitApp(document));
} else {
  hiraethInitApp(document);
}

document.addEventListener("htmx:afterSwap", (event) => {
  hiraethInitApp(event.target);
});
