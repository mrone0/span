import {
  AbsoluteFill,
  Interactive,
  interpolate,
  spring,
  useCurrentFrame,
  useVideoConfig,
} from 'remotion';
import {COLORS, MONO, enter, fade} from '../theme';

const CARD = 268;

const Node: React.FC<{
  title: string;
  sub: string;
  children: React.ReactNode;
}> = ({title, sub, children}) => {
  const frame = useCurrentFrame();
  const {fps} = useVideoConfig();
  const s = spring({frame, fps, config: {damping: 16, mass: 0.8}});
  const scale = interpolate(s, [0, 1], [0.86, 1]);

  return (
    <div
      style={{
        width: CARD,
        display: 'flex',
        flexDirection: 'column',
        alignItems: 'center',
        gap: 22,
        transform: `scale(${scale})`,
        opacity: interpolate(s, [0, 1], [0, 1]),
      }}
    >
      <div
        style={{
          width: CARD,
          height: CARD,
          borderRadius: 32,
          backgroundColor: COLORS.panel,
          border: `1px solid ${COLORS.line}`,
          display: 'flex',
          alignItems: 'center',
          justifyContent: 'center',
          boxShadow: '0 24px 70px rgba(0,0,0,0.45)',
        }}
      >
        {children}
      </div>
      <div style={{textAlign: 'center'}}>
        <div style={{fontSize: 34, fontWeight: 650, letterSpacing: 1}}>
          {title}
        </div>
        <div
          style={{
            marginTop: 8,
            fontSize: 23,
            color: COLORS.dim,
            fontFamily: MONO,
          }}
        >
          {sub}
        </div>
      </div>
    </div>
  );
};

const Wire: React.FC<{width: number}> = ({width}) => {
  const frame = useCurrentFrame();
  const grow = interpolate(frame, [0, 26], [0, 1], {
    extrapolateLeft: 'clamp',
    extrapolateRight: 'clamp',
  });

  return (
    <div
      style={{
        width,
        height: 3,
        backgroundColor: 'rgba(255,255,255,0.12)',
        borderRadius: 2,
        position: 'relative',
        overflow: 'visible',
      }}
    >
      <div
        style={{
          position: 'absolute',
          inset: 0,
          transformOrigin: 'left center',
          transform: `scaleX(${grow})`,
          backgroundColor: COLORS.accent,
          borderRadius: 2,
        }}
      />
    </div>
  );
};

const Packet = () => {
  const frame = useCurrentFrame();
  const t = interpolate(frame, [0, 60], [0, 1], {
    extrapolateLeft: 'clamp',
    extrapolateRight: 'clamp',
  });
  const opacity = interpolate(frame, [0, 6, 54, 60], [0, 1, 1, 0], {
    extrapolateLeft: 'clamp',
    extrapolateRight: 'clamp',
  });

  return (
    <div
      style={{
        position: 'absolute',
        left: 0,
        top: 0,
        width: 22,
        height: 22,
        borderRadius: '50%',
        backgroundColor: COLORS.accent,
        boxShadow: `0 0 24px ${COLORS.accent}`,
        opacity,
        transform: `translateX(${t * 1068}px) translateY(${
          -Math.sin(t * Math.PI) * 54
        }px)`,
      }}
    />
  );
};

const PhoneIcon = () => (
  <svg width={110} height={110} viewBox="0 0 110 110" fill="none">
    <rect
      x="34"
      y="16"
      width="42"
      height="78"
      rx="9"
      stroke={COLORS.fg}
      strokeWidth="5"
    />
    <rect x="46" y="25" width="18" height="4" rx="2" fill={COLORS.fg} />
    <rect x="42" y="44" width="26" height="4" rx="2" fill={COLORS.dim} />
    <rect x="42" y="54" width="18" height="4" rx="2" fill={COLORS.dim} />
    <circle cx="55" cy="83" r="4" fill={COLORS.dim} />
  </svg>
);

const LanIcon = () => (
  <svg width={116} height={116} viewBox="0 0 116 116" fill="none">
    <circle cx="58" cy="58" r="10" fill={COLORS.accent} />
    <path
      d="M38 78a28 28 0 0 1 0-40"
      stroke={COLORS.accent}
      strokeWidth="5"
      strokeLinecap="round"
    />
    <path
      d="M78 38a28 28 0 0 1 0 40"
      stroke={COLORS.accent}
      strokeWidth="5"
      strokeLinecap="round"
    />
    <path
      d="M26 90a45 45 0 0 1 0-64"
      stroke="rgba(61,220,151,0.4)"
      strokeWidth="5"
      strokeLinecap="round"
    />
    <path
      d="M90 26a45 45 0 0 1 0 64"
      stroke="rgba(61,220,151,0.4)"
      strokeWidth="5"
      strokeLinecap="round"
    />
  </svg>
);

const MonitorIcon = () => (
  <svg width={116} height={116} viewBox="0 0 116 116" fill="none">
    <rect
      x="14"
      y="22"
      width="88"
      height="58"
      rx="8"
      stroke={COLORS.fg}
      strokeWidth="5"
    />
    <path d="M44 96h28" stroke={COLORS.fg} strokeWidth="5" strokeLinecap="round" />
    <path d="M58 80v16" stroke={COLORS.fg} strokeWidth="5" />
    <rect x="30" y="56" width="24" height="5" rx="2.5" fill={COLORS.accent} />
    <rect x="30" y="44" width="44" height="5" rx="2.5" fill={COLORS.dim} />
  </svg>
);

export const Flow = () => {
  const frame = useCurrentFrame();
  const opacity = fade(frame, 315, 16, 24);

  const title = enter(frame, 6, 24);
  const note = enter(frame, 208, 24);

  return (
    <AbsoluteFill style={{opacity}}>
      <div
        style={{
          position: 'absolute',
          top: 128,
          width: '100%',
          textAlign: 'center',
          opacity: title,
          transform: `translateY(${interpolate(title, [0, 1], [26, 0])}px)`,
        }}
      >
        <div style={{fontSize: 26, color: COLORS.accent, letterSpacing: 6}}>
          工作原理
        </div>
        <div style={{marginTop: 18, fontSize: 62, fontWeight: 700, letterSpacing: 2}}>
          同一局域网，设备直连
        </div>
      </div>

      <div
        style={{
          position: 'absolute',
          top: 388,
          left: 0,
          width: '100%',
          display: 'flex',
          alignItems: 'center',
          justifyContent: 'center',
        }}
      >
        <Interactive.Div name="Phone" from={0} durationInFrames={315}>
          <Node title="手机端" sub="send text">
            <PhoneIcon />
          </Node>
        </Interactive.Div>

        <div
          style={{
            display: 'flex',
            alignItems: 'center',
            marginLeft: -6,
            marginRight: -6,
          }}
        >
          <Interactive.Div
            name="Wire 1"
            from={52}
            durationInFrames={263}
            style={{display: 'flex'}}
          >
            <Wire width={188} />
          </Interactive.Div>

          <Interactive.Div name="LAN" from={55} durationInFrames={260}>
            <Node title="局域网" sub="udp + tls">
              <LanIcon />
            </Node>
          </Interactive.Div>

          <Interactive.Div
            name="Wire 2"
            from={126}
            durationInFrames={189}
            style={{display: 'flex'}}
          >
            <Wire width={188} />
          </Interactive.Div>
        </div>

        <Interactive.Div name="PC" from={130} durationInFrames={185}>
          <Node title="PC 剪贴板" sub="system clipboard">
            <MonitorIcon />
          </Node>
        </Interactive.Div>

        <Interactive.Div
          name="Packet"
          from={160}
          durationInFrames={155}
          loop
          playbackRate={1.5}
          style={{position: 'absolute', left: 426, top: 118}}
        >
          <Packet />
        </Interactive.Div>
      </div>

      <div
        style={{
          position: 'absolute',
          bottom: 148,
          width: '100%',
          textAlign: 'center',
          opacity: note,
          transform: `translateY(${interpolate(note, [0, 1], [20, 0])}px)`,
        }}
      >
        <span
          style={{
            fontFamily: MONO,
            fontSize: 27,
            color: COLORS.dim,
            padding: '16px 34px',
            borderRadius: 999,
            border: `1px solid ${COLORS.line}`,
            backgroundColor: 'rgba(255,255,255,0.03)',
          }}
        >
          自动发现 · 加密传输 · 不经过互联网
        </span>
      </div>
    </AbsoluteFill>
  );
};
