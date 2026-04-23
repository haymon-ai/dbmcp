"use client";

import { useState } from "react";

interface InstallTab {
  label: string;
  command: string;
}

const tabs: InstallTab[] = [
  {
    label: "macOS, Linux, WSL",
    command: "curl -fsSL https://dbmcp.haymon.ai/install.sh | bash",
  },
  {
    label: "Windows PowerShell",
    command: "irm https://dbmcp.haymon.ai/install.ps1 | iex",
  },
  {
    label: "Windows CMD",
    command:
      "curl -fsSL https://dbmcp.haymon.ai/install.cmd -o install.cmd && install.cmd && del install.cmd",
  },
];

export function InstallCommand() {
  const [active, setActive] = useState(0);
  const [copied, setCopied] = useState(false);

  const handleCopy = async () => {
    try {
      await navigator.clipboard.writeText(tabs[active].command);
      setCopied(true);
      setTimeout(() => setCopied(false), 1500);
    } catch {
      // clipboard unavailable — no-op
    }
  };

  return (
    <div className="mx-auto w-full max-w-2xl">
      <div className="flex items-center gap-1 border-b border-black/[0.08]">
        {tabs.map((tab, i) => (
          <button
            key={tab.label}
            type="button"
            onClick={() => {
              setActive(i);
              setCopied(false);
            }}
            className={`cursor-pointer border-b-2 px-3 py-2 text-xs font-medium transition-colors sm:text-sm ${
              active === i
                ? "border-[#151715] text-black"
                : "border-transparent text-gray-500 hover:text-gray-800"
            }`}
          >
            {tab.label}
          </button>
        ))}
      </div>
      <div className="flex items-center justify-between gap-3 rounded-b-sm border border-t-0 border-black/[0.08] bg-gray-50 px-4 py-3">
        <code className="flex-1 overflow-x-auto whitespace-nowrap text-left font-mono text-xs text-black sm:text-sm">
          <span className="select-none text-gray-400">$ </span>
          {tabs[active].command}
        </code>
        <button
          type="button"
          onClick={handleCopy}
          aria-label="Copy install command"
          className="shrink-0 cursor-pointer rounded-sm border border-black/10 bg-white px-2.5 py-1 text-xs font-medium text-gray-700 transition-colors hover:border-black/20 hover:bg-gray-100"
        >
          {copied ? "Copied" : "Copy"}
        </button>
      </div>
    </div>
  );
}
