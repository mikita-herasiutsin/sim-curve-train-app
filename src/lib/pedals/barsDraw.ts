import { getBarFillHeight, getPercentLabel } from "./geometry";
import type { AppThemeColors } from "./theme";

export interface BarColumnLayout {
  x: number;
  width: number;
  labelY: number;
  percentY: number;
  barY: number;
  barHeight: number;
}

export interface BarsLayout {
  brake: BarColumnLayout;
  throttle: BarColumnLayout;
}

/**
 * Computes layout dimensions and positions for the two pedal bars.
 */
export function computeBarsLayout(width: number, height: number): BarsLayout {
  const paddingX = Math.max(12, width * 0.08);
  const gap = Math.max(12, width * 0.08);
  const colWidth = Math.max(20, (width - 2 * paddingX - gap) / 2);

  const percentY = Math.max(28, height * 0.09);
  const labelY = percentY + 22;
  const barY = labelY + 16;
  const barHeight = Math.max(40, height - barY - 20);

  const brakeX = paddingX;
  const throttleX = paddingX + colWidth + gap;

  return {
    brake: {
      x: brakeX,
      width: colWidth,
      percentY,
      labelY,
      barY,
      barHeight,
    },
    throttle: {
      x: throttleX,
      width: colWidth,
      percentY,
      labelY,
      barY,
      barHeight,
    },
  };
}

/**
 * Draws rounded rectangle path.
 */
function roundRect(
  ctx: CanvasRenderingContext2D,
  x: number,
  y: number,
  w: number,
  h: number,
  r: number,
): void {
  const radius = Math.min(r, w / 2, h / 2);
  if (radius <= 0) {
    ctx.rect(x, y, w, h);
    return;
  }
  ctx.beginPath();
  ctx.moveTo(x + radius, y);
  ctx.arcTo(x + w, y, x + w, y + h, radius);
  ctx.arcTo(x + w, y + h, x, y + h, radius);
  ctx.arcTo(x, y + h, x, y, radius);
  ctx.arcTo(x, y, x + w, y, radius);
  ctx.closePath();
}

/**
 * Renders the two pedal bars onto a CanvasRenderingContext2D.
 */
export function drawPedalBars(
  ctx: CanvasRenderingContext2D,
  width: number,
  height: number,
  brake: number,
  throttle: number,
  theme: AppThemeColors,
  targetPedal?: "brake" | "throttle" | "clutch",
  targetVal?: number | null,
  targetTolerance?: number | null,
): void {
  ctx.clearRect(0, 0, width, height);

  // Background
  ctx.fillStyle = theme.surface;
  ctx.fillRect(0, 0, width, height);

  const layout = computeBarsLayout(width, height);
  const columns = [
    {
      col: layout.brake,
      val: brake,
      name: "BRAKE",
      color: theme.brake,
      isTarget: targetPedal === "brake",
    },
    {
      col: layout.throttle,
      val: throttle,
      name: "THROTTLE",
      color: theme.throttle,
      isTarget: targetPedal === "throttle",
    },
  ];

  for (const item of columns) {
    const { col, val, name, color, isTarget } = item;
    const centerX = col.x + col.width / 2;
    const hasTarget = isTarget && targetVal != null;

    // 1. Large % Number
    ctx.textAlign = "center";
    ctx.textBaseline = "alphabetic";
    const percentFontSize = Math.min(32, Math.max(18, Math.floor(col.width * 0.45)));
    ctx.font = `700 ${percentFontSize}px "Inter", system-ui, sans-serif`;

    if (hasTarget) {
      // Draw two numbers side by side: Target -> Current
      const tLabel = getPercentLabel(targetVal!);
      const cLabel = getPercentLabel(val);
      const inBand = targetTolerance != null && Math.abs(val - targetVal!) <= targetTolerance;

      const valColor = inBand ? theme.accent : color;

      // We will place target on the left, current on the right
      ctx.textAlign = "right";
      ctx.fillStyle = theme.textMuted;
      ctx.fillText(`${tLabel}%`, centerX - 6, col.percentY);

      ctx.textAlign = "left";
      ctx.fillStyle = valColor;
      ctx.fillText(`${cLabel}%`, centerX + 6, col.percentY);

      ctx.textAlign = "center"; // reset for label below
    } else {
      ctx.fillStyle = color;
      ctx.fillText(`${getPercentLabel(val)}%`, centerX, col.percentY);
    }

    // 2. Label
    ctx.fillStyle = theme.textMuted;
    const labelFontSize = Math.min(13, Math.max(10, Math.floor(col.width * 0.18)));
    ctx.font = `600 ${labelFontSize}px "Inter", system-ui, sans-serif`;
    ctx.fillText(name, centerX, col.labelY);

    // 3. Bar Trough (Background)
    ctx.fillStyle = theme.surfaceRaised;
    ctx.strokeStyle = theme.border;
    ctx.lineWidth = 1;
    roundRect(ctx, col.x, col.barY, col.width, col.barHeight, 8);
    ctx.fill();
    ctx.stroke();

    // Target Band (behind the fill)
    let inBandFill = false;
    if (hasTarget && targetTolerance != null) {
      const bandTopVal = Math.min(1.0, targetVal! + targetTolerance!);
      const bandBotVal = Math.max(0.0, targetVal! - targetTolerance!);
      const topH = getBarFillHeight(bandTopVal, col.barHeight);
      const botH = getBarFillHeight(bandBotVal, col.barHeight);

      const bandY = col.barY + col.barHeight - topH;
      const bandH = topH - botH;

      inBandFill = Math.abs(val - targetVal!) <= targetTolerance!;

      ctx.save();
      roundRect(ctx, col.x, col.barY, col.width, col.barHeight, 8);
      ctx.clip();

      ctx.fillStyle = bandColor(inBandFill, theme);
      ctx.fillRect(col.x, bandY, col.width, bandH);

      ctx.restore();
    }

    // 4. Bar Fill (grows from bottom)
    const fillH = getBarFillHeight(val, col.barHeight);
    if (fillH > 0) {
      const fillY = col.barY + col.barHeight - fillH;
      ctx.save();
      // Clip to the trough shape so rounded corners look perfect
      roundRect(ctx, col.x, col.barY, col.width, col.barHeight, 8);
      ctx.clip();

      ctx.fillStyle = hasTarget && inBandFill ? theme.accent : color;
      ctx.fillRect(col.x, fillY, col.width, fillH);
      ctx.restore();
    }

    // Target Line (on top of fill)
    if (hasTarget) {
      const tH = getBarFillHeight(targetVal!, col.barHeight);
      const tY = col.barY + col.barHeight - tH;

      ctx.beginPath();
      ctx.moveTo(col.x, tY);
      ctx.lineTo(col.x + col.width, tY);
      ctx.strokeStyle = theme.text;
      ctx.lineWidth = 2;
      ctx.stroke();
    }
  }
}

/** Translucent green while the value is inside the target band, red outside it. */
export function bandColor(inBand: boolean, theme: AppThemeColors): string {
  return (inBand ? theme.throttle : theme.brake) + "50";
}
