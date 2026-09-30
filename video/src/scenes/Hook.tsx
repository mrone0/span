import {AbsoluteFill, interpolate, useCurrentFrame} from 'remotion';
import {COLORS, enter, fade} from '../theme';
import {LogoMark} from '../components/LogoMark';

const CHIPS = ['自动发现设备', '仅信任设备可见', '关掉窗口也同步'];

const Line: React.FC<{
  opacity: number;
  y: number;
  size: number;
  weight?: number;
  children: React.ReactNode;
}> = ({opacity, y, size, weight = 700, children}) => (
  <div
    style={{
      opacity,
      transform: `translateY(${y}px)`,
      fontSize: size,
      fontWeight: weight,
      lineHeight: 1.32,
      letterSpacing: 2,
      textAlign: 'center',
    }}
  >
    {children}
  </div>
);

export const Hook = () => {
  const frame = useCurrentFrame();
  const opacity = fade(frame, 255, 14, 26);

  const l1 = enter(frame, 24, 24);
  const l2 = enter(frame, 38, 24);
  const sub = enter(frame, 62, 24);

  return (
    <AbsoluteFill
      style={{
        opacity,
        alignItems: 'center',
        justifyContent: 'center',
        gap: 0,
      }}
    >
      <LogoMark size={136} />

      <div style={{height: 56}} />

      <Line opacity={l1} y={interpolate(l1, [0, 1], [36, 0])} size={96}>
        在手机上复制
      </Line>
      <Line opacity={l2} y={interpolate(l2, [0, 1], [36, 0])} size={96}>
        在电脑上粘贴
      </Line>

      <div style={{height: 40}} />

      <Line
        opacity={sub}
        y={interpolate(sub, [0, 1], [24, 0])}
        size={36}
        weight={400}
      >
        <span style={{color: COLORS.dim}}>
          同一局域网直连 · 不经过云端 · 极小占用
        </span>
      </Line>

      <div style={{height: 64}} />

      <div style={{display: 'flex', gap: 24}}>
        {CHIPS.map((chip, i) => {
          const c = enter(frame, 88 + i * 14, 20);
          return (
            <div
              key={chip}
              style={{
                opacity: c,
                transform: `translateY(${interpolate(c, [0, 1], [20, 0])}px)`,
                padding: '14px 30px',
                borderRadius: 999,
                border: `1px solid ${COLORS.line}`,
                backgroundColor: 'rgba(255,255,255,0.03)',
                color: COLORS.fg,
                fontSize: 26,
                fontWeight: 500,
                letterSpacing: 1,
              }}
            >
              {chip}
            </div>
          );
        })}
      </div>
    </AbsoluteFill>
  );
};
