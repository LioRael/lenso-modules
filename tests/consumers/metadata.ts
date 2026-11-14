import { ActionError as MetadataActionError } from "@lenso/app";
const described = app.module.describe();
if (!described.actionMetadata?.['auth.register']?.inputSchema || JSON.stringify(described.actionMetadata['auth.register'].capabilities) !== JSON.stringify(['lenso.auth-store', 'lenso.password-hasher'])) throw new Error('Installed SDK metadata missing');
try {
  await executor.callExternal(app.actions.register, context, { name: 17, password: 'private-value' });
  throw new Error('Invalid credentials unexpectedly succeeded');
} catch (error) {
  if (!(error instanceof MetadataActionError)) throw error;
  const failure = error.toJSON();
  if (failure.code !== 'invalid_input' || JSON.stringify(failure.issues) !== JSON.stringify([{ path: ['name'], code: 'invalid_value' }])) throw error;
  if (JSON.stringify(failure).includes('private-value')) throw new Error('Private input leaked');
}
