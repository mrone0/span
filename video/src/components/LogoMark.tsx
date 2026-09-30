import {Img, interpolate, spring, staticFile, useCurrentFrame} from 'remotion';

export const LogoMark: React.FC<{size?: number}> = ({size = 132}) => {
  const frame = useCurrentFrame();

  const s = spring({
    frame: frame - 4,
    fps: 30,
    config: {damping: 14, mass: 0.7, stiffness: 120},
  });

  const scale = interpolate(s, [0, 1], [0.72, 1]);
  const opacity = interpolate(frame, [4, 18], [0, 1], {
    extrapolateLeft: 'clamp',
    extrapolateRight: 'clamp',
  });

  return (
    <div
      style={{
        width: size,
        height: size,
        borderRadius: size * 0.24,
        overflow: 'hidden',
        boxShadow: '0 18px 60px rgba(0,0,0,0.5)',
        transform: `scale(${scale})`,
        opacity,
      }}
    >
      <Img
        src={staticFile('logo.png')}
        style={{width: '100%', height: '100%', display: 'block'}}
      />
    </div>
  );
};
