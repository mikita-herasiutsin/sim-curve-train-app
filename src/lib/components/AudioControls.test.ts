import { beforeEach, describe, expect, it } from "vitest";
import { fireEvent, render, screen } from "@testing-library/svelte";
import { mockIPC } from "@tauri-apps/api/mocks";
import AudioControls from "./AudioControls.svelte";

describe("AudioControls", () => {
  beforeEach(() => {
    localStorage.clear();
  });

  it("toggles audio mute and invokes audio_set_enabled", async () => {
    const invoked: Array<{ cmd: string; args: unknown }> = [];
    mockIPC((cmd, args) => {
      invoked.push({ cmd, args });
      if (cmd === "audio_set_enabled" || cmd === "audio_set_volume") return null;
      throw new Error(`unexpected command ${cmd}`);
    });

    render(AudioControls);

    const toggleBtn = screen.getByTestId("audio-toggle");
    expect(toggleBtn).toHaveAttribute("aria-pressed", "false");

    // Toggle mute
    await fireEvent.click(toggleBtn);
    expect(toggleBtn).toHaveAttribute("aria-pressed", "true");
    expect(localStorage.getItem("sct:audio_enabled")).toBe("false");

    const muteCall = invoked.find(
      (c) => c.cmd === "audio_set_enabled" && (c.args as { enabled?: boolean }).enabled === false,
    );
    expect(muteCall).toBeDefined();

    // Toggle unmute
    await fireEvent.click(toggleBtn);
    expect(toggleBtn).toHaveAttribute("aria-pressed", "false");
    expect(localStorage.getItem("sct:audio_enabled")).toBe("true");

    const unmuteCall = [...invoked]
      .reverse()
      .find(
        (c) => c.cmd === "audio_set_enabled" && (c.args as { enabled?: boolean }).enabled === true,
      );
    expect(unmuteCall).toBeDefined();
  });

  it("adjusts volume slider and invokes audio_set_volume", async () => {
    const invoked: Array<{ cmd: string; args: unknown }> = [];
    mockIPC((cmd, args) => {
      invoked.push({ cmd, args });
      if (cmd === "audio_set_enabled" || cmd === "audio_set_volume") return null;
      throw new Error(`unexpected command ${cmd}`);
    });

    render(AudioControls);

    const volumeSlider = screen.getByLabelText("Audio volume") as HTMLInputElement;
    await fireEvent.input(volumeSlider, { target: { value: "0.65" } });

    expect(localStorage.getItem("sct:audio_volume")).toBe("0.65");
    const volCall = invoked.find(
      (c) => c.cmd === "audio_set_volume" && (c.args as { volume?: number }).volume === 0.65,
    );
    expect(volCall).toBeDefined();
  });

  it("invokes audio_test_tone when clicking test tone button in dev mode", async () => {
    const invoked: Array<{ cmd: string; args: unknown }> = [];
    mockIPC((cmd, args) => {
      invoked.push({ cmd, args });
      if (cmd === "audio_set_enabled" || cmd === "audio_set_volume" || cmd === "audio_test_tone") {
        return null;
      }
      throw new Error(`unexpected command ${cmd}`);
    });

    render(AudioControls);

    const testToneBtn = screen.getByTestId("audio-test-tone");
    expect(testToneBtn).toBeInTheDocument();

    await fireEvent.click(testToneBtn);

    const toneCall = invoked.find((c) => c.cmd === "audio_test_tone");
    expect(toneCall).toBeDefined();
  });
});
