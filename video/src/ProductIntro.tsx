import {AbsoluteFill} from 'remotion';
import {COLORS, DURATION, FONT, FPS, HEIGHT, WIDTH} from './theme';
import {Background} from './components/Background';
import {Hook} from './scenes/Hook';
import {Flow} from './scenes/Flow';
import {Demo} from './scenes/Demo';
import {Outro} from './scenes/Outro';

export {DURATION, FPS, HEIGHT, WIDTH};

/*
 * 时间轴（30fps，900 帧 = 30 秒）
 *
 * 场景        from   duration   区间
 * Hook        0      255        0.0s - 8.5s   核心卖点开场
 * Flow        240    315        8.0s - 18.5s  工作原理流程
 * Demo        540    315        18.0s - 28.5s 真实使用场景
 * Outro       840    60         28.0s - 30.0s 收尾
 *
 * 相邻场景交叠 15 帧，配合各场景首尾淡入淡出做交叉转场。
 */
export const ProductIntro = () => {
  return (
    <AbsoluteFill
      style={{
        backgroundColor: COLORS.bg,
        fontFamily: FONT,
        color: COLORS.fg,
      }}
    >
      <AbsoluteFill name="Background" from={0} durationInFrames={DURATION}>
        <Background />
      </AbsoluteFill>

      <AbsoluteFill name="Hook" from={0} durationInFrames={255}>
        <Hook />
      </AbsoluteFill>

      <AbsoluteFill name="Flow" from={240} durationInFrames={315}>
        <Flow />
      </AbsoluteFill>

      <AbsoluteFill name="Demo" from={540} durationInFrames={315}>
        <Demo />
      </AbsoluteFill>

      <AbsoluteFill name="Outro" from={840} durationInFrames={60}>
        <Outro />
      </AbsoluteFill>
    </AbsoluteFill>
  );
};
