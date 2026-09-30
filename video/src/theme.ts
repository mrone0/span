import {interpolate} from 'remotion';

export const FPS = 30;
export const WIDTH = 1920;
export const HEIGHT = 1080;
export const DURATION = 900;

export const COLORS = {
  bg: '#07070d',
  panel: '#12121c',
  panelSoft: '#191926',
  fg: '#f4f4f8',
  dim: '#8e8ea3',
  accent: '#3ddc97',
  accentSoft: 'rgba(61, 220, 151, 0.14)',
  blue: '#5ac8fa',
  line: 'rgba(255,255,255,0.09)',
};

export const FONT =
  '-apple-system, BlinkMacSystemFont, "PingFang SC", "Hiragino Sans GB", "Microsoft YaHei", "Helvetica Neue", sans-serif';

export const MONO =
  'ui-monospace, "SF Mono", Menlo, Consolas, "PingFang SC", monospace';

export const fade = (
  frame: number,
  duration: number,
  fadeIn = 15,
  fadeOut = 20,
): number =>
  interpolate(
    frame,
    [0, fadeIn, Math.max(fadeIn, duration - fadeOut), duration],
    [0, 1, 1, 0],
    {
      extrapolateLeft: 'clamp',
      extrapolateRight: 'clamp',
    },
  );

export const enter = (
  frame: number,
  start: number,
  duration = 20,
): number =>
  interpolate(frame, [start, start + duration], [0, 1], {
    extrapolateLeft: 'clamp',
    extrapolateRight: 'clamp',
  });
