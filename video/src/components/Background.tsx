import {AbsoluteFill, Interactive, interpolate, useCurrentFrame} from 'remotion';
import {COLORS} from '../theme';

const Glow = () => {
  const frame = useCurrentFrame();
  const x = interpolate(frame, [0, 300, 600], [20, 80, 20]);
  const y = interpolate(frame, [0, 300, 600], [70, 30, 70]);
  const opacity = interpolate(
    frame,
    [0, 150, 300, 450, 600],
    [0.5, 0.9, 0.5, 0.9, 0.5],
  );

  return (
    <AbsoluteFill
      style={{
        background: `radial-gradient(600px 600px at ${x}% ${y}%, rgba(61, 220, 151, 0.10), transparent 70%)`,
        opacity,
      }}
    />
  );
};

export const Background = () => {
  return (
    <AbsoluteFill>
      <AbsoluteFill
        style={{
          backgroundImage:
            'radial-gradient(rgba(255,255,255,0.05) 1.2px, transparent 1.2px)',
          backgroundSize: '56px 56px',
        }}
      />
      <Interactive.Div
        loop
        durationInFrames={600}
        name="Drifting glow"
        style={{position: 'absolute', inset: 0}}
      >
        <Glow />
      </Interactive.Div>
      <AbsoluteFill
        style={{
          background:
            'radial-gradient(1200px 700px at 50% 55%, transparent 40%, rgba(7,7,13,0.75) 100%)',
        }}
      />
    </AbsoluteFill>
  );
};
