import {Composition} from 'remotion';
import {ProductIntro, DURATION, FPS, HEIGHT, WIDTH} from './ProductIntro';

export const RemotionRoot = () => {
  return (
    <Composition
      id="ProductIntro"
      component={ProductIntro}
      durationInFrames={DURATION}
      fps={FPS}
      width={WIDTH}
      height={HEIGHT}
    />
  );
};
