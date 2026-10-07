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
    },
    {
      col: layout.throttle,
      val: throttle,
      name: "THROTTLE",
      color: theme.throttle,
    },
  ];

  for (const item of columns) {
    const { col, val, name, color } = item;
    const centerX = col.x + col.width / 2;

    // 1. Large % Number
    ctx.fillStyle = color;
    ctx.textAlign = "center";
    ctx.textBaseline = "alphabetic";
    const percentFontSize = Math.min(32, Math.max(18, Math.floor(col.width * 0.45)));
    ctx.font = `700 ${percentFontSize}px "Inter", system-ui, sans-serif`;
    ctx.fillText(`${getPercentLabel(val)}%`, centerX, col.percentY);

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

    // 4. Bar Fill (grows from bottom)
    const fillH = getBarFillHeight(val, col.barHeight);
    if (fillH > 0) {
      const fillY = col.barY + col.barHeight - fillH;
      ctx.save();
      // Clip to the trough shape so rounded corners look perfect
      roundRect(ctx, col.x, col.barY, col.width, col.barHeight, 8);
      ctx.clip();

      ctx.fillStyle = color;
      ctx.fillRect(col.x, fillY, col.width, fillH);
      ctx.restore();
    }
  }
}
