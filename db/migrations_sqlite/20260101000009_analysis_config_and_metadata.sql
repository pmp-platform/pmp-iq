-- migrate:up
CREATE TABLE entity_kinds (
    id BLOB PRIMARY KEY, entity_type TEXT NOT NULL, value TEXT NOT NULL,
    UNIQUE (entity_type, value)
);

CREATE TABLE entity_properties (
    id BLOB PRIMARY KEY, entity_type TEXT NOT NULL, prop_id TEXT NOT NULL,
    name TEXT NOT NULL, data_type TEXT NOT NULL,
    UNIQUE (entity_type, prop_id)
);

-- Bring libraries/users/groups into the metadata-bearing shape.
ALTER TABLE libraries ADD COLUMN metadata TEXT NOT NULL DEFAULT '{}';
ALTER TABLE users     ADD COLUMN metadata TEXT NOT NULL DEFAULT '{}';
ALTER TABLE groups    ADD COLUMN metadata TEXT NOT NULL DEFAULT '{}';

-- Seed well-known kinds (every list includes 'other', the coercion target).
INSERT INTO entity_kinds (id, entity_type, value) VALUES
  (randomblob(16),'applications','api'),
  (randomblob(16),'applications','frontend'),
  (randomblob(16),'applications','mobile'),
  (randomblob(16),'applications','cli'),
  (randomblob(16),'applications','library'),
  (randomblob(16),'applications','service'),
  (randomblob(16),'applications','worker'),
  (randomblob(16),'applications','other'),
  (randomblob(16),'libraries','cargo'),
  (randomblob(16),'libraries','npm'),
  (randomblob(16),'libraries','pypi'),
  (randomblob(16),'libraries','maven'),
  (randomblob(16),'libraries','go'),
  (randomblob(16),'libraries','gem'),
  (randomblob(16),'libraries','composer'),
  (randomblob(16),'libraries','nuget'),
  (randomblob(16),'libraries','other'),
  (randomblob(16),'infrastructure','database'),
  (randomblob(16),'infrastructure','cache'),
  (randomblob(16),'infrastructure','queue'),
  (randomblob(16),'infrastructure','storage'),
  (randomblob(16),'infrastructure','message-broker'),
  (randomblob(16),'infrastructure','search'),
  (randomblob(16),'infrastructure','other'),
  (randomblob(16),'tools','build'),
  (randomblob(16),'tools','orchestration'),
  (randomblob(16),'tools','ci'),
  (randomblob(16),'tools','iac'),
  (randomblob(16),'tools','package-manager'),
  (randomblob(16),'tools','testing'),
  (randomblob(16),'tools','other'),
  (randomblob(16),'cloud-providers','compute'),
  (randomblob(16),'cloud-providers','storage'),
  (randomblob(16),'cloud-providers','network'),
  (randomblob(16),'cloud-providers','database'),
  (randomblob(16),'cloud-providers','serverless'),
  (randomblob(16),'cloud-providers','other'),
  (randomblob(16),'services','payments'),
  (randomblob(16),'services','auth'),
  (randomblob(16),'services','email'),
  (randomblob(16),'services','sms'),
  (randomblob(16),'services','messaging'),
  (randomblob(16),'services','api'),
  (randomblob(16),'services','other'),
  (randomblob(16),'platforms','observability'),
  (randomblob(16),'platforms','identity'),
  (randomblob(16),'platforms','ci'),
  (randomblob(16),'platforms','error-tracking'),
  (randomblob(16),'platforms','version-control'),
  (randomblob(16),'platforms','analytics'),
  (randomblob(16),'platforms','other'),
  (randomblob(16),'external','api'),
  (randomblob(16),'external','webhook'),
  (randomblob(16),'external','cdn'),
  (randomblob(16),'external','dataset'),
  (randomblob(16),'external','other');

-- Seed a standard property set per entity type.
INSERT INTO entity_properties (id, entity_type, prop_id, name, data_type) VALUES
  (randomblob(16),'applications','language_version','Language Version','string'),
  (randomblob(16),'applications','framework','Framework','string'),
  (randomblob(16),'applications','repository_url','Repository URL','string'),
  (randomblob(16),'applications','team','Team','string'),
  (randomblob(16),'applications','deployment_target','Deployment Target','string'),
  (randomblob(16),'libraries','license','License','string'),
  (randomblob(16),'libraries','purpose','Purpose','string'),
  (randomblob(16),'infrastructure','provider','Provider','string'),
  (randomblob(16),'infrastructure','region','Region','string'),
  (randomblob(16),'tools','config_file','Config File','string'),
  (randomblob(16),'cloud-providers','region','Region','string'),
  (randomblob(16),'cloud-providers','services_used','Services Used','array_of_strings'),
  (randomblob(16),'services','provider','Provider','string'),
  (randomblob(16),'services','auth_method','Auth Method','string'),
  (randomblob(16),'services','base_url','Base URL','string'),
  (randomblob(16),'platforms','plan_tier','Plan Tier','string'),
  (randomblob(16),'platforms','region','Region','string'),
  (randomblob(16),'external','url','URL','string'),
  (randomblob(16),'users','full_name','Full Name','string'),
  (randomblob(16),'users','role','Role','string'),
  (randomblob(16),'groups','description','Description','string');

-- migrate:down
