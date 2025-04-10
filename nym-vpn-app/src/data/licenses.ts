import { CodeDependency, JsLicensesJson, RustLicensesJson } from '../types';

// These files are generated by `npm run gen:licenses` command
// and are located in the `public` directory
const LicensesJs = '/licenses-js.json';
const LicensesRust = '/licenses-rust.json';

export async function getRustLicenses(): Promise<CodeDependency[] | undefined> {
  let json: RustLicensesJson;
  try {
    const response = await fetch(LicensesRust);
    json = (await response.json()) as RustLicensesJson;
  } catch (e) {
    if (import.meta.env.MODE === 'production') {
      console.warn('Failed to fetch Rust licenses data', e);
    }
    return;
  }

  let list: CodeDependency[] = [];
  try {
    list = json.map((info) => {
      return {
        ...info,
        authors: info.authors?.split('|') || ['-'],
        licenses: [info.license],
      };
    });
  } catch (e) {
    console.warn('Failed to parse Rust licenses data', e);
    return;
  }

  return list;
}

export async function getJsLicenses(): Promise<CodeDependency[] | undefined> {
  let json: JsLicensesJson;
  try {
    const response = await fetch(LicensesJs);
    json = (await response.json()) as JsLicensesJson;
  } catch (e) {
    if (import.meta.env.MODE === 'production') {
      console.warn('Failed to fetch Js licenses data', e);
    }
    return;
  }

  let list: CodeDependency[] = [];
  try {
    list = Object.entries(json).map(([name, info]) => {
      let licenses: string[] = [];
      if (info.licenses) {
        if (Array.isArray(info.licenses)) {
          licenses = [...info.licenses];
        } else {
          licenses = [info.licenses];
        }
      }
      // package name is formatted as `name@semver`
      const components = name.split('@');
      const version = components.pop() || '0.0.0';

      return {
        ...info,
        name: components.join('@'),
        version,
        authors: info.publisher ? [info.publisher] : ['-'],
        licenses,
      };
    });
  } catch (e) {
    console.warn('Failed to parse Js licenses data', e);
    return;
  }

  return list;
}
