import {AbsoluteFill, Interactive, interpolate, useCurrentFrame} from 'remotion';
import {COLORS} from '../theme';
import {LogoMark} from '../components/LogoMark';

export const Outro = () => {
  const frame = useCurrentFrame();
  const opacity = interpolate(frame, [0, 14], [0, 1], {
    extrapolateLeft: 'clamp',
    extrapolateRight: 'clamp',
  });
  const text = interpolate(frame, [16, 34], [0, 1], {
    extrapolateLeft: 'clamp',
    extrapolateRight: 'clamp',
  });

  return (
    <AbsoluteFill
      style={{
        opacity,
        alignItems: 'center',
        justifyContent: 'center',
      }}
    >
      <Interactive.Div name="Logo" trimBefore={24}>
        <LogoMark size={116} />
      </Interactive.Div>

      <div
        style={{
          marginTop: 34,
          fontSize: 76,
          fontWeight: 750,
          letterSpacing: 10,
          opacity: text,
          transform: `translateY(${interpolate(text, [0, 1], [18, 0])}px)`,
        }}
      >
        span
      </div>
      <div
        style={{
          marginTop: 14,
          fontSize: 27,
          color: COLORS.dim,
          letterSpacing: 5,
          opacity: text,
        }}
      >
        跨设备文本 · 局域网直连
      </div>
    </AbsoluteFill>
  );
};
