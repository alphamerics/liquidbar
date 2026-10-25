declare module "@babel/standalone" {
  export interface BabelTransformResult {
    code?: string | null;
  }
  export function transform(code: string, options: unknown): BabelTransformResult;
}
