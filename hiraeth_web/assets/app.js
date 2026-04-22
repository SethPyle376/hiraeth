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
