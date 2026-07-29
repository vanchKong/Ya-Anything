/// <reference types="vite/client" />

declare module "*.vue" {
  import type { DefineComponent } from "vue";
  const component: DefineComponent<{}, {}, any>;
  export default component;
}

declare module "draco3dgltf" {
  export function createDecoderModule(config?: {
    locateFile?: (file: string) => string;
  }): Promise<unknown>;
  export function createEncoderModule(config?: {
    locateFile?: (file: string) => string;
  }): Promise<unknown>;
}
