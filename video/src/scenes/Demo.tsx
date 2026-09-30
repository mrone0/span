import {
  AbsoluteFill,
  Interactive,
  interpolate,
  spring,
  useCurrentFrame,
  useVideoConfig,
} from 'remotion';
import {COLORS, MONO, enter, fade} from '../theme';

const TEXT = 'v1.2 已同步：新增 Quick Settings Tile，手机复制即达 PC';

const Toast = () => {
  const frame = useCurrentFrame();
  const {fps} = useVideoConfig();
  const s = spring({
    frame: frame - 68,
    fps,
    config: {damping: 13, mass: 0.6},
  });
  const opacity = interpolate(frame, [68, 78], [0, 1], {
    extrapolateLeft: 'clamp',
    extrapolateRight: 'clamp',
  });

  return (
    <div
      style={{
        position: 'absolute',
        top: 478,
        left: 0,
        width: '100%',
        display: 'flex',
        justifyContent: 'center',
        opacity,
        transform: `translateY(${interpolate(s, [0, 1], [-16, 0])}px)`,
      }}
    >
      <div
        style={{
          padding: '12px 28px',
          borderRadius: 999,
          backgroundColor: COLORS.accent,
          color: '#05130d',
          fontSize: 24,
          fontWeight: 700,
          letterSpacing: 2,
        }}
      >
        已复制
      </div>
    </div>
  );
};

const Flyer = () => {
  const frame = useCurrentFrame();
  const t = interpolate(frame, [0, 52], [0, 1], {
    extrapolateLeft: 'clamp',
    extrapolateRight: 'clamp',
  });
  const opacity = interpolate(frame, [0, 6, 46, 52], [0, 1, 1, 0], {
    extrapolateLeft: 'clamp',
    extrapolateRight: 'clamp',
  });

  return (
    <div
      style={{
        position: 'absolute',
        left: 0,
        top: 0,
        width: 26,
        height: 26,
        borderRadius: 7,
        backgroundColor: COLORS.accent,
        boxShadow: `0 0 30px ${COLORS.accent}`,
        opacity,
        transform: `translateX(${t * 460}px) translateY(${
          -Math.sin(t * Math.PI) * 120
        }px) rotate(${t * 220}deg)`,
      }}
    />
  );
};

export const Demo = () => {
  const frame = useCurrentFrame();
  const {fps} = useVideoConfig();
  const opacity = fade(frame, 315, 16, 24);

  const rise = (start: number) => {
    const s = spring({frame: frame - start, fps, config: {damping: 18}});
    return {
      opacity: interpolate(s, [0, 1], [0, 1]),
      transform: `translateY(${interpolate(s, [0, 1], [46, 0])}px)`,
    };
  };

  const phoneStyle = rise(4);
  const deskStyle = rise(18);
  const selection = enter(frame, 42, 24);
  const caption = enter(frame, 104, 24);
  const cursorOn = frame >= 132;
  const typed = TEXT.slice(
    0,
    Math.max(0, Math.floor((frame - 142) / 3)),
  );
  const arrived = enter(frame, 246, 18);

  return (
    <AbsoluteFill style={{opacity}}>
      <div
        style={{
          position: 'absolute',
          left: 300,
          top: 236,
          width: 300,
          height: 570,
          borderRadius: 42,
          border: `1px solid ${COLORS.line}`,
          backgroundColor: COLORS.panel,
          boxShadow: '0 40px 100px rgba(0,0,0,0.55)',
          overflow: 'hidden',
          ...phoneStyle,
        }}
      >
        <div
          style={{
            height: 54,
            display: 'flex',
            alignItems: 'center',
            justifyContent: 'space-between',
            padding: '0 26px',
            fontSize: 20,
            color: COLORS.dim,
            fontFamily: MONO,
          }}
        >
          <span>9:41</span>
          <span>span</span>
        </div>

        <div style={{padding: '8px 26px 0'}}>
          <div
            style={{
              fontSize: 18,
              color: COLORS.dim,
              letterSpacing: 3,
              marginBottom: 14,
            }}
          >
            分享文本
          </div>
          <div
            style={{
              position: 'relative',
              padding: 6,
              borderRadius: 12,
            }}
          >
            <div
              style={{
                position: 'absolute',
                inset: 0,
                borderRadius: 12,
                backgroundColor: 'rgba(61, 220, 151, 0.22)',
                opacity: selection,
              }}
            />
            <div
              style={{
                position: 'relative',
                fontSize: 25,
                lineHeight: 1.65,
                color: COLORS.fg,
              }}
            >
              {TEXT}
            </div>
          </div>
        </div>

        <Toast />
      </div>

      <div
        style={{
          position: 'absolute',
          left: 940,
          top: 318,
          width: 760,
          height: 456,
          borderRadius: 20,
          border: `1px solid ${
            frame >= 142 && frame < 176
              ? 'rgba(61, 220, 151, 0.55)'
              : COLORS.line
          }`,
          backgroundColor: COLORS.panel,
          boxShadow: '0 40px 100px rgba(0,0,0,0.55)',
          overflow: 'hidden',
          ...deskStyle,
        }}
      >
        <div
          style={{
            height: 56,
            display: 'flex',
            alignItems: 'center',
            gap: 10,
            padding: '0 22px',
            borderBottom: `1px solid ${COLORS.line}`,
          }}
        >
          <div style={{width: 13, height: 13, borderRadius: '50%', backgroundColor: '#ff5f57'}} />
          <div style={{width: 13, height: 13, borderRadius: '50%', backgroundColor: '#febc2e'}} />
          <div style={{width: 13, height: 13, borderRadius: '50%', backgroundColor: '#28c840'}} />
          <div
            style={{
              marginLeft: 16,
              flex: 1,
              height: 30,
              borderRadius: 8,
              backgroundColor: 'rgba(255,255,255,0.05)',
              display: 'flex',
              alignItems: 'center',
              padding: '0 14px',
              fontSize: 19,
              color: COLORS.dim,
              fontFamily: MONO,
            }}
          >
            notes.txt
          </div>
        </div>

        <div style={{padding: '34px 38px'}}>
          <div
            style={{
              fontFamily: MONO,
              fontSize: 27,
              lineHeight: 1.75,
              color: COLORS.fg,
              minHeight: 150,
            }}
          >
            {typed}
            {cursorOn ? (
              <span
                style={{
                  display: 'inline-block',
                  width: 14,
                  height: 30,
                  marginLeft: 4,
                  verticalAlign: 'text-bottom',
                  backgroundColor: COLORS.accent,
                  opacity: Math.floor(frame / 8) % 2 === 0 ? 1 : 0.15,
                }}
              />
            ) : null}
          </div>

          <div
            style={{
              marginTop: 34,
              display: 'flex',
              alignItems: 'center',
              gap: 12,
              opacity: arrived,
              fontSize: 22,
              color: COLORS.accent,
              fontFamily: MONO,
            }}
          >
            <span>✓</span>
            <span>received from phone · LAN</span>
          </div>
        </div>
      </div>

      <Interactive.Div
        name="Flyer"
        from={94}
        durationInFrames={54}
        style={{position: 'absolute', left: 636, top: 452}}
      >
        <Flyer />
      </Interactive.Div>

      <div
        style={{
          position: 'absolute',
          bottom: 140,
          width: '100%',
          textAlign: 'center',
          opacity: caption,
          transform: `translateY(${interpolate(caption, [0, 1], [20, 0])}px)`,
        }}
      >
        <span style={{fontSize: 40, fontWeight: 650, letterSpacing: 3}}>
          手机复制
          <span style={{color: COLORS.accent, margin: '0 22px'}}>→</span>
          电脑直接粘贴
        </span>
      </div>
    </AbsoluteFill>
  );
};
