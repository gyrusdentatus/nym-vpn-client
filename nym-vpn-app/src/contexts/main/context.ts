import { createContext, useContext } from 'react';
import { AppState } from '../../types';
import { StateAction, initialState } from './reducer';

export const MainStateContext = createContext<AppState>(initialState);
export const MainDispatchContext =
  createContext<React.Dispatch<StateAction> | null>(null);

export const useMainState = () => {
  return useContext(MainStateContext);
};

export const useMainDispatch = () => {
  return useContext(MainDispatchContext);
};
