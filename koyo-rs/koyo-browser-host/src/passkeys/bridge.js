// The passkey bridge. The renderer runs it in each frame's main world as the frame's script
// context is made, before any of the page's scripts (renderer.rs). It takes the page's passkey
// requests (WebAuthn's navigator.credentials.create and get with publicKey) and the
// availability checks to the browser host, which checks them against the frame's origin and
// asks Koyo; every other kind of credential (password, federated) goes to Chromium as before.
//
// `send(op, json, id)` is the host's native function, kept in this closure so the page never
// sees it: it sends a request and returns its number, or 0 when it didn't go. What this
// returns is what the renderer calls back: `receive(json)`, how the host answers, with
// `{id, ok}` or `{id, error}`; and `install()`.
//
// What the page gets back is built from WebAuthn's JSON forms (RegistrationResponseJSON,
// AuthenticationResponseJSON) on PublicKeyCredential's own prototypes, so `instanceof` holds
// and every member a site reads is there. The constructors and functions the bridge relies
// on most are taken now, before a page can change them; a page that changes the rest only
// breaks its own sign-in, as the host checks every request anyway.
//
// The engine adds the secure-only interfaces (CredentialsContainer, PublicKeyCredential and
// the rest) just after this runs, so the bridge goes in a moment later (`install`): as the
// frame's document starts loading, before its first script, when the renderer calls it.
// A document that has no load of its own (a frame a script makes and writes into) gets it the
// first time the page reaches for navigator.credentials, through a getter of the frame's own
// navigator that goes once the bridge is in, or as soon as the page's first task is over.
((send) => {
  'use strict';
  if (!globalThis.isSecureContext) return null;
  let Container = null;
  let Credential = null;
  let Attestation = null;
  let Assertion = null;
  const { create: objectCreate, defineProperty, getOwnPropertyDescriptor, keys } = Object;
  const { apply } = Reflect;
  const { stringify, parse } = JSON;
  const { isView } = ArrayBuffer;
  const tagOf = Object.prototype.toString;
  const [Promise_, DOMException_, TypeError_, Uint8Array_, Array_] =
    [Promise, DOMException, TypeError, Uint8Array, Array];
  const { toBase64 } = Uint8Array.prototype;
  const { fromBase64 } = Uint8Array;
  const BASE64URL = { alphabet: 'base64url', omitPadding: true };
  const notAllowed = () => new DOMException_(
    'The operation either timed out or was not allowed. See: ' +
      'https://www.w3.org/TR/webauthn-2/#sctn-privacy-considerations-client.',
    'NotAllowedError');

  // Requests waiting on the host, by number: what settles each with the host's answer.
  const pending = objectCreate(null);

  const errorOf = ({ name, message }) => name === 'TypeError'
    ? new TypeError_(message) : new DOMException_(message, name);

  // Asks the host for what `build` makes of the page's options (a throw there, a TypeError
  // say, rejects at once); the promise settles with `make(ok)` once it answers, or with the
  // signal's reason as soon as the page aborts.
  const ask = (op, build, signal, make) => new Promise_((resolve, reject) => {
    const options = build();
    if (signal && signal.aborted) {
      reject(signal.reason);
      return;
    }
    const id = send(op, options === undefined ? '' : stringify(options), 0);
    if (!id) {
      reject(notAllowed());
      return;
    }
    let onAbort = null;
    const done = () => {
      delete pending[id];
      if (onAbort) signal.removeEventListener('abort', onAbort);
    };
    pending[id] = (answer) => {
      done();
      if (answer.error) {
        reject(errorOf(answer.error));
        return;
      }
      try {
        resolve(make(answer.ok));
      } catch {
        reject(notAllowed());
      }
    };
    if (signal) {
      onAbort = () => {
        done();
        send('cancel', '', id);
        reject(signal.reason);
      };
      signal.addEventListener('abort', onAbort);
    }
  });
  const nothing = () => undefined;

  // A conditional request (passkeys offered in the page's autofill), which Koyo has no
  // autofill to offer in: it waits, as a browser with no passkeys to offer leaves it, until
  // the page aborts it.
  const never = (signal) => new Promise_((_, reject) => {
    if (!signal) return;
    if (signal.aborted) {
      reject(signal.reason);
      return;
    }
    signal.addEventListener('abort', () => reject(signal.reason), { once: true });
  });

  const base64url = (source, what) => {
    let bytes;
    if (isView(source)) {
      bytes = new Uint8Array_(source.buffer, source.byteOffset, source.byteLength);
    } else if (apply(tagOf, source, []) === '[object ArrayBuffer]') {
      bytes = new Uint8Array_(source);
    } else {
      throw new TypeError_(`${what} is not of type '(ArrayBuffer or ArrayBufferView)'.`);
    }
    return apply(toBase64, bytes, [BASE64URL]);
  };
  const buffer = (text) => apply(fromBase64, Uint8Array_, [String(text), BASE64URL]).buffer;
  const clone = (value) => (value === undefined ? undefined : parse(stringify(value)));
  const text = (value) => (value === undefined || value === null ? undefined : String(value));
  const required = (dictionary, member, what) => {
    if (dictionary === null || typeof dictionary !== 'object' || dictionary[member] === undefined) {
      throw new TypeError_(`Failed to read the '${member}' property from '${what}': ` +
        'Required member is undefined.');
    }
    return dictionary[member];
  };
  const strings = (list) => (list === undefined || list === null ? [] : Array_.from(list, String));
  // Descriptors of credentials; entries of a type other than public-key are skipped.
  const descriptors = (list, what) => Array_.from(list || [], (descriptor, at) => (
    descriptor && descriptor.type === 'public-key'
      ? { id: base64url(descriptor.id, `${what}[${at}].id`), transports: strings(descriptor.transports) }
      : null)).filter(Boolean);

  const creation = (options) => {
    const what = 'PublicKeyCredentialCreationOptions';
    const rp = required(options, 'rp', what);
    const user = required(options, 'user', what);
    const challenge = required(options, 'challenge', what);
    const params = Array_.from(required(options, 'pubKeyCredParams', what));
    const algorithms = params
      .filter((param) => param && param.type === 'public-key')
      .map((param) => ({ type: 'public-key', alg: Math.trunc(Number(param.alg)) || 0 }));
    if (params.length > 0 && algorithms.length === 0) {
      throw new DOMException_('None of the algorithms asked for are supported.', 'NotSupportedError');
    }
    const selection = options.authenticatorSelection || {};
    const residentKey = text(selection.residentKey) ||
      (selection.requireResidentKey === true ? 'required' : undefined);
    return {
      rp: { id: text(rp.id) },
      user: {
        id: base64url(required(user, 'id', 'PublicKeyCredentialUserEntity'), 'user.id'),
        name: text(user.name) || '',
        displayName: text(user.displayName) || '',
      },
      challenge: base64url(challenge, 'challenge'),
      pubKeyCredParams: algorithms,
      excludeCredentials: descriptors(options.excludeCredentials, 'excludeCredentials'),
      authenticatorSelection: {
        authenticatorAttachment: text(selection.authenticatorAttachment),
        residentKey,
        userVerification: text(selection.userVerification),
      },
      attestation: text(options.attestation),
      extensions: { credProps: !!(options.extensions && options.extensions.credProps) },
    };
  };

  const request = (options) => ({
    challenge: base64url(required(options, 'challenge', 'PublicKeyCredentialRequestOptions'), 'challenge'),
    rpId: text(options.rpId),
    allowCredentials: descriptors(options.allowCredentials, 'allowCredentials'),
    userVerification: text(options.userVerification),
  });

  // Members of a credential or its response, as its own: they stand in front of the
  // prototype's getters and methods, which only take the engine's own objects.
  const own = (object, values) => {
    for (const key of keys(values)) {
      defineProperty(object, key, { value: values[key], configurable: true });
    }
    return object;
  };
  const credential = (json) => {
    const answer = json.response;
    let response;
    if (answer.attestationObject !== undefined) {
      const transports = strings(answer.transports);
      response = own(objectCreate(Attestation.prototype), {
        clientDataJSON: buffer(answer.clientDataJSON),
        attestationObject: buffer(answer.attestationObject),
        getAuthenticatorData() { return buffer(answer.authenticatorData); },
        getPublicKey() {
          return answer.publicKey === undefined ? null : buffer(answer.publicKey);
        },
        getPublicKeyAlgorithm() { return answer.publicKeyAlgorithm; },
        getTransports() { return Array_.from(transports); },
      });
    } else {
      response = own(objectCreate(Assertion.prototype), {
        clientDataJSON: buffer(answer.clientDataJSON),
        authenticatorData: buffer(answer.authenticatorData),
        signature: buffer(answer.signature),
        userHandle: answer.userHandle === undefined ? null : buffer(answer.userHandle),
      });
    }
    return own(objectCreate(Credential.prototype), {
      id: json.id,
      rawId: buffer(json.rawId),
      type: 'public-key',
      authenticatorAttachment: json.authenticatorAttachment || null,
      response,
      getClientExtensionResults() { return clone(json.clientExtensionResults) || {}; },
      toJSON() { return clone(json); },
    });
  };

  // Puts `method` in the place of `object[name]`, as it was defined; the one it replaces.
  const replace = (object, name, method) => {
    const was = getOwnPropertyDescriptor(object, name);
    if (!was || typeof was.value !== 'function') return null;
    defineProperty(object, name, {
      value: method,
      writable: was.writable,
      enumerable: was.enumerable,
      configurable: was.configurable,
    });
    return was.value;
  };

  const passkey = (options) => options !== null && typeof options === 'object' &&
    options.publicKey !== undefined;
  let chromiumCreate = null;
  let chromiumGet = null;
  // Defaults keep each method's length at 0, as the engine's own.
  const methods = {
    create(options = undefined) {
      if (!(this instanceof Container) || !passkey(options)) {
        return apply(chromiumCreate, this, arguments);
      }
      return ask('create', () => creation(options.publicKey), options.signal, credential);
    },
    get(options = undefined) {
      if (!(this instanceof Container) || !passkey(options)) {
        return apply(chromiumGet, this, arguments);
      }
      if (options.mediation === 'conditional') return never(options.signal);
      return ask('get', () => request(options.publicKey), options.signal, credential);
    },
  };
  const statics = {
    isUserVerifyingPlatformAuthenticatorAvailable() {
      return ask('available', nothing, null, Boolean);
    },
    isConditionalMediationAvailable() { return Promise_.resolve(false); },
    getClientCapabilities() { return ask('capabilities', nothing, null, clone); },
    // Signals about a site's passkeys go to the system's store only with a sheet; there's
    // nothing to tell it otherwise, and Chromium's own store isn't Koyo's.
    signalUnknownCredential() { return Promise_.resolve(); },
    signalAllAcceptedCredentials() { return Promise_.resolve(); },
    signalCurrentUserDetails() { return Promise_.resolve(); },
  };
  // Puts the bridge in the engine's place once the interfaces are there: whether it is.
  let installed = false;
  const install = () => {
    if (installed) return true;
    ({
      CredentialsContainer: Container,
      PublicKeyCredential: Credential,
      AuthenticatorAttestationResponse: Attestation,
      AuthenticatorAssertionResponse: Assertion,
    } = globalThis);
    if (!Container || !Credential || !Attestation || !Assertion) return false;
    installed = true;
    try {
      chromiumCreate = replace(Container.prototype, 'create', methods.create);
      chromiumGet = replace(Container.prototype, 'get', methods.get);
      for (const name of keys(statics)) replace(Credential, name, statics[name]);
    } catch {
      // The page keeps the engine's own.
    }
    return true;
  };
  // The getter standing in for navigator's own until the bridge is in, then gone.
  const navigator_ = globalThis.navigator;
  let hooked = false;
  const settle = () => {
    if (!install() || !hooked) return;
    hooked = false;
    delete navigator_.credentials;
  };
  if (!install() && navigator_) {
    defineProperty(navigator_, 'credentials', {
      get: function credentials() {
        settle();
        return hooked ? undefined : navigator_.credentials;
      },
      configurable: true,
    });
    hooked = true;
    setTimeout(settle, 0);
  }

  const receive = (json) => {
    let answer;
    try {
      answer = parse(json);
    } catch {
      return;
    }
    const answered = pending[answer.id];
    if (answered) answered(answer);
  };
  return { receive, install: settle };
})
